use globset::{Glob, GlobSet, GlobSetBuilder};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{AesMode, CompressionMethod, ZipWriter};

use crate::backup::{Status, BACKUPS_DIR};
use crate::config::ExclusionSettings;
use crate::presets::ResolvedAgent;

const MAX_WARNINGS: usize = 20;
const COPY_CHUNK: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct PackedSource {
    pub key: String,
    pub source_type: &'static str,
    pub label: String,
    pub enabled_paths: Option<Vec<String>>,
    pub root_path: PathBuf,
    pub archive_name: String,
    pub archive_path: PathBuf,
    pub bytes: u64,
    /// Digest over every packed file's relative path and content; independent of
    /// compression and encryption, so equal digests mean equal content.
    pub sha256: String,
    /// Digest of the zip file as written.
    pub archive_sha256: String,
    pub file_count: u64,
    pub status: Status,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// One source with its file list already collected, so the estimate and the
/// packing step share a single directory walk.
pub struct SourcePlan {
    pub key: String,
    pub source_type: &'static str,
    pub label: String,
    pub enabled_paths: Option<Vec<String>>,
    pub root_path: PathBuf,
    pub bytes: u64,
    archive_stem: String,
    collected: Result<Collected, String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceEstimate {
    pub key: String,
    pub label: String,
    pub file_count: u64,
    pub bytes: u64,
}

impl SourcePlan {
    fn new(
        key: &str,
        source_type: &'static str,
        label: &str,
        enabled_paths: Option<Vec<String>>,
        root_path: PathBuf,
        archive_stem: &str,
        collected: Result<Collected, String>,
    ) -> Self {
        let bytes = collected
            .as_ref()
            .map(|c| {
                c.files
                    .iter()
                    .filter_map(|(abs, _)| fs::metadata(abs).ok())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0);
        Self {
            key: key.to_string(),
            source_type,
            label: label.to_string(),
            enabled_paths,
            root_path,
            bytes,
            archive_stem: archive_stem.to_string(),
            collected,
        }
    }

    pub fn estimate(&self) -> SourceEstimate {
        SourceEstimate {
            key: self.key.clone(),
            label: self.label.clone(),
            file_count: self.collected.as_ref().map_or(0, |c| c.files.len() as u64),
            bytes: self.bytes,
        }
    }

    pub fn pack(
        self,
        work_dir: &Path,
        timestamp: &str,
        password: Option<&str>,
        on_bytes: &mut dyn FnMut(u64),
    ) -> PackedSource {
        let archive_name = format!("{}_{}.zip", self.archive_stem, timestamp);
        let mut out = PackedSource {
            key: self.key,
            source_type: self.source_type,
            label: self.label,
            enabled_paths: self.enabled_paths,
            root_path: self.root_path,
            archive_path: work_dir.join(&archive_name),
            archive_name,
            bytes: 0,
            sha256: String::new(),
            archive_sha256: String::new(),
            file_count: 0,
            status: Status::Skipped,
            error: None,
            warnings: Vec::new(),
        };
        let collected = match self.collected {
            Ok(c) => c,
            Err(e) => {
                out.status = Status::Failed;
                out.error = Some(e);
                return out;
            }
        };
        out.warnings = collected.warnings;
        if !collected.files.is_empty() {
            match zip_files(&collected.files, &out.archive_path, password, on_bytes) {
                Ok(z) => {
                    out.warnings.extend(z.warnings);
                    if z.file_count == 0 {
                        let first = out.warnings.first().cloned().unwrap_or_default();
                        out.status = Status::Failed;
                        out.error = Some(t!(
                            "所有文件都读取失败：{first}",
                            "Every file failed to read: {first}"
                        ));
                    } else {
                        out.bytes = z.bytes;
                        out.sha256 = z.sha256;
                        out.archive_sha256 = z.archive_sha256;
                        out.file_count = z.file_count;
                        out.status = Status::Ok;
                    }
                }
                Err(e) => {
                    out.status = Status::Failed;
                    out.error = Some(e);
                }
            }
        }
        cap_warnings(&mut out.warnings);
        out
    }
}

pub fn plan_agent(
    agent: &ResolvedAgent,
    enabled_group_ids: &[String],
    exclusions: &ExclusionSettings,
    skip_dirs: &[PathBuf],
) -> SourcePlan {
    let includes: Vec<String> = agent
        .groups
        .iter()
        .filter(|g| enabled_group_ids.iter().any(|id| id == &g.id))
        .flat_map(|g| g.include.iter().cloned())
        .collect();
    let collected = match &agent.root_path {
        None => Err(t!("根目录未解析", "Root folder could not be resolved")),
        Some(root) => Rules::new(exclusions, &agent.hard_exclude, skip_dirs)
            .and_then(|rules| collect_files(root, &includes, &rules)),
    };
    SourcePlan::new(
        &agent.key,
        "agent",
        &agent.label,
        Some(enabled_group_ids.to_vec()),
        agent.root_path.clone().unwrap_or_default(),
        &agent.key,
        collected,
    )
}

pub fn plan_custom(
    id: &str,
    label: &str,
    path: &Path,
    archive_stem: &str,
    exclusions: &ExclusionSettings,
    skip_dirs: &[PathBuf],
) -> SourcePlan {
    let collected = if !path.exists() {
        Err(t!("路径不存在", "Path does not exist"))
    } else if is_within(path, skip_dirs) {
        Err(t!(
            "位于备份目标的 backups 目录内，已跳过",
            "Inside a destination's backups folder; skipped"
        ))
    } else {
        Rules::new(exclusions, &[], skip_dirs).and_then(|rules| collect_custom(path, &rules))
    };
    SourcePlan::new(
        id,
        "custom",
        label,
        None,
        path.to_path_buf(),
        archive_stem,
        collected,
    )
}

fn cap_warnings(warnings: &mut Vec<String>) {
    if warnings.len() > MAX_WARNINGS {
        let extra = warnings.len() - MAX_WARNINGS;
        warnings.truncate(MAX_WARNINGS);
        warnings.push(t!("……另有 {extra} 条", "…and {extra} more"));
    }
}

#[derive(Debug, Default)]
pub struct Collected {
    pub files: Vec<(PathBuf, String)>,
    pub warnings: Vec<String>,
}

pub fn build_exclusion_set(
    exclusions: &ExclusionSettings,
    hard_exclude: &[String],
) -> Result<(GlobSet, GlobSet), String> {
    let mut file_builder = GlobSetBuilder::new();
    let mut dir_builder = GlobSetBuilder::new();

    for pattern in exclusions.file_name_globs.iter().chain(hard_exclude.iter()) {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.ends_with('/') {
            let name = trimmed.trim_end_matches('/');
            dir_builder.add(Glob::new(name).map_err(|e| e.to_string())?);
            dir_builder.add(Glob::new(trimmed).map_err(|e| e.to_string())?);
        } else if trimmed.contains('/') {
            // path-ish hard exclude handled later by relative prefix
            file_builder.add(
                Glob::new(trimmed.trim_start_matches('/')).map_err(|e| e.to_string())?,
            );
        } else {
            file_builder.add(Glob::new(trimmed).map_err(|e| e.to_string())?);
            // also treat bare names as dir names
            dir_builder.add(Glob::new(trimmed).map_err(|e| e.to_string())?);
        }
    }

    for pattern in &exclusions.dir_name_globs {
        dir_builder.add(Glob::new(pattern).map_err(|e| e.to_string())?);
    }

    Ok((
        file_builder.build().map_err(|e| e.to_string())?,
        dir_builder.build().map_err(|e| e.to_string())?,
    ))
}

fn should_skip_dir(name: &str, dir_globs: &GlobSet) -> bool {
    dir_globs.is_match(name)
}

fn should_skip_file(name: &str, relative: &str, file_globs: &GlobSet, hard: &[String]) -> bool {
    if file_globs.is_match(name) || file_globs.is_match(relative) {
        return true;
    }
    for pattern in hard {
        let p = pattern.trim_start_matches("./");
        if p.ends_with('/') {
            let prefix = p.trim_end_matches('/');
            if relative == prefix || relative.starts_with(&format!("{prefix}/")) {
                return true;
            }
        } else if relative == p || name == p {
            return true;
        }
    }
    false
}

fn walk_error(err: &walkdir::Error) -> String {
    match err.path() {
        Some(p) => t!("跳过 {}: {err}", "Skipped {}: {err}", p.display()),
        None => t!("跳过: {err}", "Skipped: {err}"),
    }
}

/// Whether `path` resolves to one of `dirs` (already canonical) or somewhere below it.
pub fn is_within(path: &Path, dirs: &[PathBuf]) -> bool {
    !dirs.is_empty()
        && fs::canonicalize(path).is_ok_and(|p| dirs.iter().any(|d| p.starts_with(d)))
}

/// Everything a walk leaves out.
struct Rules<'a> {
    file_globs: GlobSet,
    dir_globs: GlobSet,
    hard_exclude: &'a [String],
    /// Canonical `backups/` directories of local destinations; packing them would
    /// re-archive earlier backups on every run.
    skip_dirs: &'a [PathBuf],
}

impl<'a> Rules<'a> {
    fn new(
        exclusions: &ExclusionSettings,
        hard_exclude: &'a [String],
        skip_dirs: &'a [PathBuf],
    ) -> Result<Self, String> {
        let (file_globs, dir_globs) = build_exclusion_set(exclusions, hard_exclude)?;
        Ok(Self {
            file_globs,
            dir_globs,
            hard_exclude,
            skip_dirs,
        })
    }

    fn skips_dir(&self, entry: &walkdir::DirEntry) -> bool {
        let name = entry.file_name().to_string_lossy();
        should_skip_dir(&name, &self.dir_globs)
            || (name == BACKUPS_DIR && is_within(entry.path(), self.skip_dirs))
    }

    fn skips_file(&self, name: &str, relative: &str) -> bool {
        should_skip_file(name, relative, &self.file_globs, self.hard_exclude)
    }
}

/// Walks `dir`, appending files (relative to `root`) that survive the exclusion rules.
fn walk_into(out: &mut Collected, root: &Path, dir: &Path, rules: &Rules) -> Result<(), String> {
    let walker = WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_type().is_dir() || !rules.skips_dir(e));
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                out.warnings.push(walk_error(&err));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let abs = entry.path().to_path_buf();
        let rel = abs
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let name = entry.file_name().to_string_lossy().to_string();
        if rules.skips_file(&name, &rel) {
            continue;
        }
        out.files.push((abs, rel));
    }
    Ok(())
}

fn collect_files(root: &Path, includes: &[String], rules: &Rules) -> Result<Collected, String> {
    let mut out = Collected::default();

    for include in includes {
        let include = include.replace('\\', "/");
        let target = root.join(include.trim_end_matches('/'));
        if !target.exists() || is_within(&target, rules.skip_dirs) {
            continue;
        }
        if target.is_file() {
            let rel = include.trim_start_matches('/').to_string();
            let name = target
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if !rules.skips_file(&name, &rel) {
                out.files.push((target, rel));
            }
            continue;
        }
        walk_into(&mut out, root, &target, rules)?;
    }

    out.files.sort_by(|a, b| a.1.cmp(&b.1));
    out.files.dedup_by(|a, b| a.1 == b.1);
    Ok(out)
}

fn collect_files_tree(root: &Path, rules: &Rules) -> Result<Collected, String> {
    let mut out = Collected::default();
    walk_into(&mut out, root, root, rules)?;
    out.files.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

fn collect_custom(path: &Path, rules: &Rules) -> Result<Collected, String> {
    if path.is_dir() {
        return collect_files_tree(path, rules);
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    collect_files(path.parent().unwrap_or(path), &[name], rules)
}

pub struct ZipOutcome {
    pub bytes: u64,
    pub sha256: String,
    pub archive_sha256: String,
    pub file_count: u64,
    pub warnings: Vec<String>,
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| t!("读取 zip 失败: {e}", "Failed to read zip: {e}"))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| t!("读取 zip 失败: {e}", "Failed to read zip: {e}"))?;
    Ok(hex::encode(hasher.finalize()))
}

/// Streams each file into the archive. Files that can't be read are skipped and
/// reported in `warnings`; `on_bytes` receives raw bytes as they are consumed.
pub fn zip_files(
    files: &[(PathBuf, String)],
    out_path: &Path,
    password: Option<&str>,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<ZipOutcome, String> {
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = File::create(out_path).map_err(|e| t!("创建 zip 失败: {e}", "Failed to create zip: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let mut options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    if let Some(pw) = password {
        options = options.with_aes_encryption(AesMode::Aes256, pw);
    }
    let mut hasher = Sha256::new();
    let mut file_count: u64 = 0;
    let mut warnings = Vec::new();
    let mut buf = vec![0u8; COPY_CHUNK];

    for (abs, rel) in files {
        let mut src = match File::open(abs) {
            Ok(f) => f,
            Err(e) => {
                warnings.push(t!("读取失败 {rel}: {e}", "Failed to read {rel}: {e}"));
                continue;
            }
        };
        let len = src.metadata().map(|m| m.len()).unwrap_or(0);
        zip.start_file(rel, options.large_file(len >= u32::MAX as u64))
            .map_err(|e| t!("写入 zip entry 失败: {e}", "Failed to write zip entry: {e}"))?;
        let mut file_hasher = Sha256::new();
        let mut read_error = None;
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    read_error = Some(e);
                    break;
                }
            };
            file_hasher.update(&buf[..n]);
            zip.write_all(&buf[..n])
                .map_err(|e| t!("压缩写入失败: {e}", "Compression failed: {e}"))?;
            on_bytes(n as u64);
        }
        if let Some(e) = read_error {
            zip.abort_file()
                .map_err(|e| t!("撤销 zip 条目失败: {e}", "Failed to roll back zip entry: {e}"))?;
            warnings.push(t!("读取失败 {rel}: {e}", "Failed to read {rel}: {e}"));
            continue;
        }
        hasher.update(rel.as_bytes());
        hasher.update(file_hasher.finalize());
        file_count += 1;
    }

    zip.finish().map_err(|e| t!("完成 zip 失败: {e}", "Failed to finish zip: {e}"))?;
    Ok(ZipOutcome {
        bytes: fs::metadata(out_path).map(|m| m.len()).unwrap_or(0),
        sha256: hex::encode(hasher.finalize()),
        archive_sha256: sha256_file(out_path)?,
        file_count,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn temp_base(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("agent-backup-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn link_dir(target: &Path, link: &Path) {
        #[cfg(windows)]
        {
            let out = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .expect("run mklink");
            assert!(out.status.success(), "mklink failed: {out:?}");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).expect("symlink");
    }

    fn rels(files: &[(PathBuf, String)]) -> Vec<&str> {
        files.iter().map(|(_, r)| r.as_str()).collect()
    }

    fn default_rules() -> Rules<'static> {
        Rules::new(&AppConfig::default().exclusions, &[], &[]).unwrap()
    }

    #[test]
    fn destination_backups_dir_inside_a_source_is_not_packed() {
        let base = temp_base("recursion");
        let output = base.join("dest").join("backups").join("host");
        fs::create_dir_all(&output).unwrap();
        fs::write(output.join("old.zip"), "z").unwrap();
        fs::create_dir_all(base.join("other").join("backups")).unwrap();
        fs::write(base.join("other").join("backups").join("keep.md"), "k").unwrap();
        fs::write(base.join("notes.md"), "n").unwrap();
        let skip = vec![fs::canonicalize(base.join("dest").join("backups")).unwrap()];
        let excl = AppConfig::default().exclusions;

        let plan = plan_custom("c", "C", &base, "c", &excl, &skip);
        let files = &plan.collected.as_ref().unwrap().files;
        assert_eq!(rels(files), vec!["notes.md", "other/backups/keep.md"]);

        let inside = plan_custom("c", "C", &output, "c", &excl, &skip);
        assert!(inside.collected.is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn collect_files_follows_directory_links() {
        let base = temp_base("link");
        let shared = base.join("shared").join("skill-a");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("SKILL.md"), "a").unwrap();
        let skills = base.join("root").join("skills");
        fs::create_dir_all(&skills).unwrap();
        fs::write(skills.join("local.md"), "b").unwrap();
        link_dir(&shared, &skills.join("skill-a"));

        let files = collect_files(&base.join("root"), &["skills/".into()], &default_rules()).unwrap();
        assert_eq!(rels(&files.files), vec!["skills/local.md", "skills/skill-a/SKILL.md"]);

        let tree = collect_files_tree(&base.join("root"), &default_rules()).unwrap();
        assert_eq!(rels(&tree.files), vec!["skills/local.md", "skills/skill-a/SKILL.md"]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn explicit_include_is_not_dropped_by_global_dir_excludes() {
        let base = temp_base("depth0");
        fs::create_dir_all(base.join("tmp").join("chat").join("tmp")).unwrap();
        fs::write(base.join("tmp").join("chat").join("log.json"), "a").unwrap();
        fs::write(base.join("tmp").join("chat").join("tmp").join("x"), "b").unwrap();

        let files = collect_files(&base, &["tmp/".into()], &default_rules()).unwrap();
        assert_eq!(rels(&files.files), vec!["tmp/chat/log.json"]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn collect_files_survives_link_loops() {
        let base = temp_base("loop");
        let skills = base.join("skills");
        fs::create_dir_all(&skills).unwrap();
        fs::write(skills.join("a.md"), "a").unwrap();
        link_dir(&base, &skills.join("loop"));

        let files = collect_files(&base, &["skills/".into()], &default_rules()).unwrap();
        assert!(files.files.iter().any(|(_, r)| r == "skills/a.md"));
        assert!(files.files.len() < 10, "loop was expanded: {:?}", rels(&files.files));
        assert!(!files.warnings.is_empty(), "loop should be reported");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn broken_link_and_unreadable_file_become_warnings() {
        let base = temp_base("warn");
        let root = base.join("root");
        fs::create_dir_all(root.join("skills")).unwrap();
        fs::write(root.join("skills").join("ok.md"), "ok").unwrap();
        fs::write(root.join("skills").join("locked.db"), "x").unwrap();
        let gone = base.join("gone");
        fs::create_dir_all(&gone).unwrap();
        link_dir(&gone, &root.join("skills").join("dangling"));
        fs::remove_dir_all(&gone).unwrap();

        let collected = collect_files(&root, &["skills/".into()], &default_rules());
        let plan = SourcePlan::new("t", "agent", "T", None, root.clone(), "t", collected);

        #[cfg(windows)]
        let _lock = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(root.join("skills").join("locked.db"))
                .unwrap()
        };
        let packed = plan.pack(&base, "ts", None, &mut |_| {});

        assert_eq!(packed.status, Status::Ok, "{:?}", packed.error);
        assert!(packed.warnings.iter().any(|w| w.contains("dangling")), "{:?}", packed.warnings);
        #[cfg(windows)]
        {
            assert_eq!(packed.file_count, 1);
            assert!(packed.warnings.iter().any(|w| w.contains("locked.db")), "{:?}", packed.warnings);
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn content_digest_ignores_encryption_and_archive_digest_matches_file() {
        let base = temp_base("digest");
        fs::write(base.join("a.txt"), "hello").unwrap();
        let files = vec![(base.join("a.txt"), "a.txt".to_string())];
        let plain = zip_files(&files, &base.join("plain.zip"), None, &mut |_| {}).unwrap();
        let enc = zip_files(&files, &base.join("enc.zip"), Some("pw"), &mut |_| {}).unwrap();
        assert_eq!(plain.sha256, enc.sha256);
        assert_ne!(plain.archive_sha256, enc.archive_sha256);
        assert_eq!(plain.archive_sha256, sha256_file(&base.join("plain.zip")).unwrap());

        let mut zip = zip::ZipArchive::new(File::open(base.join("plain.zip")).unwrap()).unwrap();
        let mut text = String::new();
        zip.by_name("a.txt").unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "hello");
        drop(zip);
        let _ = fs::remove_dir_all(&base);
    }
}
