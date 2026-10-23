use globset::{Glob, GlobSet, GlobSetBuilder};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{AesMode, CompressionMethod, ZipWriter};

use crate::config::ExclusionSettings;
use crate::presets::ResolvedAgent;

const MAX_WARNINGS: usize = 20;

#[derive(Debug, Clone)]
pub struct PackedSource {
    pub key: String,
    pub source_type: String,
    pub label: String,
    pub enabled_paths: Option<Vec<String>>,
    pub root_path: PathBuf,
    pub archive_name: String,
    pub archive_path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
    pub file_count: u64,
    pub status: String,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

impl PackedSource {
    fn new(
        key: &str,
        source_type: &str,
        label: &str,
        enabled_paths: Option<Vec<String>>,
        root_path: PathBuf,
        work_dir: &Path,
        archive_name: String,
    ) -> Self {
        Self {
            key: key.to_string(),
            source_type: source_type.to_string(),
            label: label.to_string(),
            enabled_paths,
            root_path,
            archive_path: work_dir.join(&archive_name),
            archive_name,
            bytes: 0,
            sha256: String::new(),
            file_count: 0,
            status: "skipped".into(),
            error: None,
            warnings: Vec::new(),
        }
    }

    fn failed(mut self, error: String) -> Self {
        self.status = "failed".into();
        self.error = Some(error);
        self
    }

    fn pack(
        mut self,
        collected: Result<Collected, String>,
        password: Option<&str>,
        on_file: &mut dyn FnMut(u64),
    ) -> Self {
        let collected = match collected {
            Ok(c) => c,
            Err(e) => return self.failed(e),
        };
        self.warnings = collected.warnings;
        if !collected.files.is_empty() {
            match zip_files(&collected.files, &self.archive_path, password, on_file) {
                Ok(out) => {
                    self.warnings.extend(out.warnings);
                    if out.file_count == 0 {
                        let first = self.warnings.first().cloned().unwrap_or_default();
                        self = self.failed(format!("所有文件都读取失败：{first}"));
                    } else {
                        self.bytes = out.bytes;
                        self.sha256 = out.sha256;
                        self.file_count = out.file_count;
                        self.status = "ok".into();
                    }
                }
                Err(e) => self = self.failed(e),
            }
        }
        cap_warnings(&mut self.warnings);
        self
    }
}

fn cap_warnings(warnings: &mut Vec<String>) {
    if warnings.len() > MAX_WARNINGS {
        let extra = warnings.len() - MAX_WARNINGS;
        warnings.truncate(MAX_WARNINGS);
        warnings.push(format!("……另有 {extra} 条"));
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
        Some(p) => format!("跳过 {}: {err}", p.display()),
        None => format!("跳过: {err}"),
    }
}

/// Walks `dir`, appending files (relative to `root`) that survive the exclusion rules.
fn walk_into(
    out: &mut Collected,
    root: &Path,
    dir: &Path,
    file_globs: &GlobSet,
    dir_globs: &GlobSet,
    hard_exclude: &[String],
) -> Result<(), String> {
    let walker = WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 || !e.file_type().is_dir() {
                return true;
            }
            !should_skip_dir(&e.file_name().to_string_lossy(), dir_globs)
        });
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
        if should_skip_file(&name, &rel, file_globs, hard_exclude) {
            continue;
        }
        out.files.push((abs, rel));
    }
    Ok(())
}

fn collect_files(
    root: &Path,
    includes: &[String],
    exclusions: &ExclusionSettings,
    hard_exclude: &[String],
) -> Result<Collected, String> {
    let (file_globs, dir_globs) = build_exclusion_set(exclusions, hard_exclude)?;
    let mut out = Collected::default();

    for include in includes {
        let include = include.replace('\\', "/");
        let target = root.join(include.trim_end_matches('/'));
        if !target.exists() {
            continue;
        }
        if target.is_file() {
            let rel = include.trim_start_matches('/').to_string();
            let name = target
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if !should_skip_file(&name, &rel, &file_globs, hard_exclude) {
                out.files.push((target, rel));
            }
            continue;
        }
        walk_into(&mut out, root, &target, &file_globs, &dir_globs, hard_exclude)?;
    }

    out.files.sort_by(|a, b| a.1.cmp(&b.1));
    out.files.dedup_by(|a, b| a.1 == b.1);
    Ok(out)
}

fn collect_files_tree(
    root: &Path,
    exclusions: &ExclusionSettings,
    hard_exclude: &[String],
) -> Result<Collected, String> {
    let (file_globs, dir_globs) = build_exclusion_set(exclusions, hard_exclude)?;
    let mut out = Collected::default();
    walk_into(&mut out, root, root, &file_globs, &dir_globs, hard_exclude)?;
    out.files.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

fn collect_custom(path: &Path, exclusions: &ExclusionSettings) -> Result<Collected, String> {
    if path.is_dir() {
        return collect_files_tree(path, exclusions, &[]);
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    collect_files(path.parent().unwrap_or(path), &[name], exclusions, &[])
}

fn enabled_includes(agent: &ResolvedAgent, enabled_group_ids: &[String]) -> Vec<String> {
    agent
        .groups
        .iter()
        .filter(|g| enabled_group_ids.iter().any(|id| id == &g.id))
        .flat_map(|g| g.include.iter().cloned())
        .collect()
}

pub struct ZipOutcome {
    pub bytes: u64,
    pub sha256: String,
    pub file_count: u64,
    pub warnings: Vec<String>,
}

pub fn zip_files(
    files: &[(PathBuf, String)],
    out_path: &Path,
    password: Option<&str>,
    on_file: &mut dyn FnMut(u64),
) -> Result<ZipOutcome, String> {
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = File::create(out_path).map_err(|e| format!("创建 zip 失败: {e}"))?;
    let mut zip = ZipWriter::new(file);
    let mut options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    if let Some(pw) = password {
        options = options.with_aes_encryption(AesMode::Aes256, pw);
    }
    let mut hasher = Sha256::new();
    let mut total_bytes: u64 = 0;
    let mut file_count: u64 = 0;
    let mut warnings = Vec::new();
    let mut buffer = Vec::new();

    for (abs, rel) in files {
        buffer.clear();
        if let Err(e) = File::open(abs).and_then(|mut f| f.read_to_end(&mut buffer)) {
            warnings.push(format!("读取失败 {rel}: {e}"));
            continue;
        }
        zip.start_file(rel, options)
            .map_err(|e| format!("写入 zip entry 失败: {e}"))?;
        zip.write_all(&buffer)
            .map_err(|e| format!("压缩写入失败: {e}"))?;
        hasher.update(rel.as_bytes());
        hasher.update(&buffer);
        total_bytes += buffer.len() as u64;
        file_count += 1;
        on_file(buffer.len() as u64);
    }

    zip.finish().map_err(|e| format!("完成 zip 失败: {e}"))?;
    Ok(ZipOutcome {
        bytes: fs::metadata(out_path).map(|m| m.len()).unwrap_or(total_bytes),
        sha256: hex::encode(hasher.finalize()),
        file_count,
        warnings,
    })
}

pub fn pack_agent(
    agent: &ResolvedAgent,
    enabled_group_ids: &[String],
    exclusions: &ExclusionSettings,
    work_dir: &Path,
    timestamp: &str,
    password: Option<&str>,
    on_file: &mut dyn FnMut(u64),
) -> PackedSource {
    let item = PackedSource::new(
        &agent.key,
        "agent",
        &agent.label,
        Some(enabled_group_ids.to_vec()),
        agent.root_path.clone().unwrap_or_default(),
        work_dir,
        format!("{}_{}.zip", agent.key, timestamp),
    );
    let Some(root) = &agent.root_path else {
        return item.failed("根目录未解析".into());
    };
    let includes = enabled_includes(agent, enabled_group_ids);
    if includes.is_empty() {
        return item;
    }
    item.pack(
        collect_files(root, &includes, exclusions, &agent.hard_exclude),
        password,
        on_file,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn pack_custom(
    id: &str,
    label: &str,
    path: &Path,
    exclusions: &ExclusionSettings,
    work_dir: &Path,
    timestamp: &str,
    slug: &str,
    password: Option<&str>,
    on_file: &mut dyn FnMut(u64),
) -> PackedSource {
    let item = PackedSource::new(
        id,
        "custom",
        label,
        None,
        path.to_path_buf(),
        work_dir,
        format!("{}_{}.zip", slug, timestamp),
    );
    if !path.exists() {
        return item.failed("路径不存在".into());
    }
    item.pack(collect_custom(path, exclusions), password, on_file)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceEstimate {
    pub key: String,
    pub label: String,
    pub file_count: u64,
    pub bytes: u64,
}

fn estimate_of(key: &str, label: &str, files: &[(PathBuf, String)]) -> SourceEstimate {
    let bytes = files
        .iter()
        .filter_map(|(abs, _)| fs::metadata(abs).ok())
        .map(|m| m.len())
        .sum();
    SourceEstimate {
        key: key.to_string(),
        label: label.to_string(),
        file_count: files.len() as u64,
        bytes,
    }
}

pub fn estimate_agent(
    agent: &ResolvedAgent,
    enabled_group_ids: &[String],
    exclusions: &ExclusionSettings,
) -> Result<SourceEstimate, String> {
    let root = agent
        .root_path
        .as_deref()
        .ok_or_else(|| "根目录未解析".to_string())?;
    let includes = enabled_includes(agent, enabled_group_ids);
    let files = if includes.is_empty() {
        Vec::new()
    } else {
        collect_files(root, &includes, exclusions, &agent.hard_exclude)?.files
    };
    Ok(estimate_of(&agent.key, &agent.label, &files))
}

pub fn estimate_custom(
    id: &str,
    label: &str,
    path: &Path,
    exclusions: &ExclusionSettings,
) -> Result<SourceEstimate, String> {
    let files = if path.exists() {
        collect_custom(path, exclusions)?.files
    } else {
        Vec::new()
    };
    Ok(estimate_of(id, label, &files))
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

        let excl = AppConfig::default().exclusions;
        let files = collect_files(&base.join("root"), &["skills/".into()], &excl, &[]).unwrap();
        assert_eq!(rels(&files.files), vec!["skills/local.md", "skills/skill-a/SKILL.md"]);

        let tree = collect_files_tree(&base.join("root"), &excl, &[]).unwrap();
        assert_eq!(rels(&tree.files), vec!["skills/local.md", "skills/skill-a/SKILL.md"]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn explicit_include_is_not_dropped_by_global_dir_excludes() {
        let base = temp_base("depth0");
        fs::create_dir_all(base.join("tmp").join("chat").join("tmp")).unwrap();
        fs::write(base.join("tmp").join("chat").join("log.json"), "a").unwrap();
        fs::write(base.join("tmp").join("chat").join("tmp").join("x"), "b").unwrap();

        let excl = AppConfig::default().exclusions;
        let files = collect_files(&base, &["tmp/".into()], &excl, &[]).unwrap();
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

        let excl = AppConfig::default().exclusions;
        let files = collect_files(&base, &["skills/".into()], &excl, &[]).unwrap();
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

        let item = PackedSource::new("t", "agent", "T", None, root.clone(), &base, "t.zip".into());
        let collected = collect_files(&root, &["skills/".into()], &AppConfig::default().exclusions, &[]);

        #[cfg(windows)]
        let _lock = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(root.join("skills").join("locked.db"))
                .unwrap()
        };
        let packed = item.pack(collected, None, &mut |_| {});

        assert_eq!(packed.status, "ok", "{:?}", packed.error);
        assert!(packed.warnings.iter().any(|w| w.contains("dangling")), "{:?}", packed.warnings);
        #[cfg(windows)]
        {
            assert_eq!(packed.file_count, 1);
            assert!(packed.warnings.iter().any(|w| w.contains("locked.db")), "{:?}", packed.warnings);
        }
        drop(packed);
        let _ = fs::remove_dir_all(&base);
    }
}
