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

fn collect_files(
    root: &Path,
    includes: &[String],
    exclusions: &ExclusionSettings,
    hard_exclude: &[String],
) -> Result<Vec<(PathBuf, String)>, String> {
    let (file_globs, dir_globs) = build_exclusion_set(exclusions, hard_exclude)?;
    let mut files = Vec::new();

    for include in includes {
        let include = include.replace('\\', "/");
        let target = if include.ends_with('/') {
            root.join(include.trim_end_matches('/'))
        } else {
            root.join(&include)
        };
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
                files.push((target, rel));
            }
            continue;
        }

        for entry in WalkDir::new(&target)
            .into_iter()
            .filter_entry(|e| {
                if e.file_type().is_dir() {
                    let name = e.file_name().to_string_lossy();
                    !should_skip_dir(&name, &dir_globs)
                } else {
                    true
                }
            })
            .filter_map(|e| e.ok())
        {
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
            if should_skip_file(&name, &rel, &file_globs, hard_exclude) {
                continue;
            }
            files.push((abs, rel));
        }
    }

    files.sort_by(|a, b| a.1.cmp(&b.1));
    files.dedup_by(|a, b| a.1 == b.1);
    Ok(files)
}

pub fn zip_files(
    files: &[(PathBuf, String)],
    out_path: &Path,
    password: Option<&str>,
) -> Result<(u64, String, u64), String> {
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
    let mut buffer = Vec::new();

    for (abs, rel) in files {
        zip.start_file(rel, options)
            .map_err(|e| format!("写入 zip entry 失败: {e}"))?;
        let mut f = File::open(abs).map_err(|e| format!("读取文件失败 {}: {e}", abs.display()))?;
        buffer.clear();
        f.read_to_end(&mut buffer)
            .map_err(|e| format!("读取文件失败 {}: {e}", abs.display()))?;
        zip.write_all(&buffer)
            .map_err(|e| format!("压缩写入失败: {e}"))?;
        hasher.update(rel.as_bytes());
        hasher.update(&buffer);
        total_bytes += buffer.len() as u64;
        file_count += 1;
    }

    zip.finish().map_err(|e| format!("完成 zip 失败: {e}"))?;
    let digest = hex::encode(hasher.finalize());
    let meta_bytes = fs::metadata(out_path).map(|m| m.len()).unwrap_or(total_bytes);
    Ok((meta_bytes, digest, file_count))
}

pub fn pack_agent(
    agent: &ResolvedAgent,
    enabled_group_ids: &[String],
    exclusions: &ExclusionSettings,
    work_dir: &Path,
    timestamp: &str,
    password: Option<&str>,
) -> PackedSource {
    let archive_name = format!("{}_{}.zip", agent.key, timestamp);
    let archive_path = work_dir.join(&archive_name);
    let root = match &agent.root_path {
        Some(p) => p.clone(),
        None => {
            return PackedSource {
                key: agent.key.clone(),
                source_type: "agent".into(),
                label: agent.label.clone(),
                enabled_paths: Some(enabled_group_ids.to_vec()),
                root_path: PathBuf::new(),
                archive_name,
                archive_path,
                bytes: 0,
                sha256: String::new(),
                file_count: 0,
                status: "failed".into(),
                error: Some("根目录未解析".into()),
            };
        }
    };

    let mut includes = Vec::new();
    for group in &agent.groups {
        if enabled_group_ids.iter().any(|id| id == &group.id) {
            includes.extend(group.include.clone());
        }
    }

    if includes.is_empty() {
        return PackedSource {
            key: agent.key.clone(),
            source_type: "agent".into(),
            label: agent.label.clone(),
            enabled_paths: Some(enabled_group_ids.to_vec()),
            root_path: root,
            archive_name,
            archive_path,
            bytes: 0,
            sha256: String::new(),
            file_count: 0,
            status: "skipped".into(),
            error: None,
        };
    }

    match collect_files(&root, &includes, exclusions, &agent.hard_exclude) {
        Ok(files) if files.is_empty() => PackedSource {
            key: agent.key.clone(),
            source_type: "agent".into(),
            label: agent.label.clone(),
            enabled_paths: Some(enabled_group_ids.to_vec()),
            root_path: root,
            archive_name,
            archive_path,
            bytes: 0,
            sha256: String::new(),
            file_count: 0,
            status: "skipped".into(),
            error: None,
        },
        Ok(files) => match zip_files(&files, &archive_path, password) {
            Ok((bytes, sha256, file_count)) => PackedSource {
                key: agent.key.clone(),
                source_type: "agent".into(),
                label: agent.label.clone(),
                enabled_paths: Some(enabled_group_ids.to_vec()),
                root_path: root,
                archive_name,
                archive_path,
                bytes,
                sha256,
                file_count,
                status: "ok".into(),
                error: None,
            },
            Err(e) => PackedSource {
                key: agent.key.clone(),
                source_type: "agent".into(),
                label: agent.label.clone(),
                enabled_paths: Some(enabled_group_ids.to_vec()),
                root_path: root,
                archive_name,
                archive_path,
                bytes: 0,
                sha256: String::new(),
                file_count: 0,
                status: "failed".into(),
                error: Some(e),
            },
        },
        Err(e) => PackedSource {
            key: agent.key.clone(),
            source_type: "agent".into(),
            label: agent.label.clone(),
            enabled_paths: Some(enabled_group_ids.to_vec()),
            root_path: root,
            archive_name,
            archive_path,
            bytes: 0,
            sha256: String::new(),
            file_count: 0,
            status: "failed".into(),
            error: Some(e),
        },
    }
}

pub fn pack_custom(
    id: &str,
    label: &str,
    path: &Path,
    exclusions: &ExclusionSettings,
    work_dir: &Path,
    timestamp: &str,
    slug: &str,
    password: Option<&str>,
) -> PackedSource {
    let archive_name = format!("{}_{}.zip", slug, timestamp);
    let archive_path = work_dir.join(&archive_name);
    if !path.exists() {
        return PackedSource {
            key: id.to_string(),
            source_type: "custom".into(),
            label: label.to_string(),
            enabled_paths: None,
            root_path: path.to_path_buf(),
            archive_name,
            archive_path,
            bytes: 0,
            sha256: String::new(),
            file_count: 0,
            status: "failed".into(),
            error: Some("路径不存在".into()),
        };
    }

    let includes = if path.is_file() {
        vec![path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string()]
    } else {
        // pack directory contents with relative paths under that root
        vec!["./".to_string()]
    };

    // For directory custom sources, walk the directory itself as root
    let (root, include_list) = if path.is_dir() {
        (path.to_path_buf(), vec!["".to_string()])
    } else {
        (
            path.parent().unwrap_or(path).to_path_buf(),
            includes,
        )
    };

    let files = if path.is_dir() {
        collect_files_tree(&root, exclusions, &[])
    } else {
        collect_files(&root, &include_list, exclusions, &[])
    };

    match files {
        Ok(files) if files.is_empty() => PackedSource {
            key: id.to_string(),
            source_type: "custom".into(),
            label: label.to_string(),
            enabled_paths: None,
            root_path: path.to_path_buf(),
            archive_name,
            archive_path,
            bytes: 0,
            sha256: String::new(),
            file_count: 0,
            status: "skipped".into(),
            error: None,
        },
        Ok(files) => match zip_files(&files, &archive_path, password) {
            Ok((bytes, sha256, file_count)) => PackedSource {
                key: id.to_string(),
                source_type: "custom".into(),
                label: label.to_string(),
                enabled_paths: None,
                root_path: path.to_path_buf(),
                archive_name,
                archive_path,
                bytes,
                sha256,
                file_count,
                status: "ok".into(),
                error: None,
            },
            Err(e) => PackedSource {
                key: id.to_string(),
                source_type: "custom".into(),
                label: label.to_string(),
                enabled_paths: None,
                root_path: path.to_path_buf(),
                archive_name,
                archive_path,
                bytes: 0,
                sha256: String::new(),
                file_count: 0,
                status: "failed".into(),
                error: Some(e),
            },
        },
        Err(e) => PackedSource {
            key: id.to_string(),
            source_type: "custom".into(),
            label: label.to_string(),
            enabled_paths: None,
            root_path: path.to_path_buf(),
            archive_name,
            archive_path,
            bytes: 0,
            sha256: String::new(),
            file_count: 0,
            status: "failed".into(),
            error: Some(e),
        },
    }
}

fn collect_files_tree(
    root: &Path,
    exclusions: &ExclusionSettings,
    hard_exclude: &[String],
) -> Result<Vec<(PathBuf, String)>, String> {
    let (file_globs, dir_globs) = build_exclusion_set(exclusions, hard_exclude)?;
    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            if e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                !should_skip_dir(&name, &dir_globs)
            } else {
                true
            }
        })
        .filter_map(|e| e.ok())
    {
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
        if should_skip_file(&name, &rel, &file_globs, hard_exclude) {
            continue;
        }
        files.push((abs, rel));
    }
    files.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(files)
}
