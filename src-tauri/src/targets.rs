use std::fs;
use std::path::{Path, PathBuf};

use crate::backup::{Status, BACKUPS_DIR};
use crate::config::{Destination, DestinationKind, LocalDestination, S3Destination};
use crate::packager::PackedSource;

/// Where one backup run is written. The backup runner only talks to this trait, so a
/// new destination type needs a config variant and an implementation here.
pub trait Target {
    /// Writes every successfully packed archive and returns the run's location.
    fn write_archives(
        &self,
        hostname: &str,
        timestamp: &str,
        packed: &[PackedSource],
    ) -> Result<String, String>;

    /// Writes `manifest.json` next to the archives of a run `write_archives` completed.
    fn write_manifest(&self, hostname: &str, timestamp: &str, manifest: &[u8]) -> Result<(), String>;

    /// Local directory that holds this target's backups, which sources must never pack.
    fn output_dir(&self) -> Option<PathBuf> {
        None
    }
}

impl Destination {
    pub fn target(&self) -> &dyn Target {
        match &self.kind {
            DestinationKind::Local { local } => local,
            DestinationKind::S3 { s3 } => s3,
        }
    }
}

impl LocalDestination {
    fn run_dir(&self, hostname: &str, timestamp: &str) -> PathBuf {
        PathBuf::from(&self.root_path)
            .join(BACKUPS_DIR)
            .join(hostname)
            .join(timestamp)
    }
}

impl Target for LocalDestination {
    fn write_archives(
        &self,
        hostname: &str,
        timestamp: &str,
        packed: &[PackedSource],
    ) -> Result<String, String> {
        write_dir_atomically(&self.run_dir(hostname, timestamp), packed)
    }

    fn write_manifest(&self, hostname: &str, timestamp: &str, manifest: &[u8]) -> Result<(), String> {
        fs::write(self.run_dir(hostname, timestamp).join("manifest.json"), manifest)
            .map_err(|e| e.to_string())
    }

    fn output_dir(&self) -> Option<PathBuf> {
        Some(PathBuf::from(&self.root_path).join(BACKUPS_DIR))
    }
}

impl Target for S3Destination {
    fn write_archives(
        &self,
        hostname: &str,
        timestamp: &str,
        packed: &[PackedSource],
    ) -> Result<String, String> {
        crate::s3::upload_archives(self, hostname, timestamp, packed)
    }

    fn write_manifest(&self, hostname: &str, timestamp: &str, manifest: &[u8]) -> Result<(), String> {
        crate::s3::upload_manifest(self, hostname, timestamp, manifest)
    }
}

/// Copies into `<target>.partial` and renames when complete, so an interrupted
/// copy never looks like a finished backup.
fn write_dir_atomically(target: &Path, packed: &[PackedSource]) -> Result<String, String> {
    let mut partial_name = target.file_name().unwrap_or_default().to_os_string();
    partial_name.push(".partial");
    let partial = target.with_file_name(partial_name);
    let copy_all = || -> Result<(), String> {
        fs::create_dir_all(&partial).map_err(|e| t!("创建目标目录失败: {e}", "Failed to create destination folder: {e}"))?;
        for p in packed.iter().filter(|p| p.status == Status::Ok) {
            fs::copy(&p.archive_path, partial.join(&p.archive_name))
                .map_err(|e| t!("复制 {} 失败: {e}", "Failed to copy {}: {e}", p.archive_name))?;
        }
        fs::rename(&partial, target).map_err(|e| t!("完成目标目录失败: {e}", "Failed to finalize destination folder: {e}"))
    };
    if let Err(e) = copy_all() {
        let _ = fs::remove_dir_all(&partial);
        return Err(e);
    }
    Ok(target.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agent-backup-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn packed(key: &str, archive_path: PathBuf) -> PackedSource {
        PackedSource {
            key: key.into(),
            source_type: "agent",
            label: key.into(),
            enabled_paths: None,
            root_path: PathBuf::new(),
            archive_name: format!("{key}.zip"),
            archive_path,
            bytes: 1,
            sha256: String::new(),
            archive_sha256: String::new(),
            file_count: 1,
            status: Status::Ok,
            error: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn local_write_is_all_or_nothing() {
        let work = temp_dir("work");
        fs::write(work.join("a.zip"), "a").unwrap();
        let ok = packed("a", work.join("a.zip"));
        let missing = packed("b", work.join("missing.zip"));
        let dest = LocalDestination {
            root_path: temp_dir("dest").to_string_lossy().into(),
        };
        let host_dir = PathBuf::from(&dest.root_path).join(BACKUPS_DIR).join("host");

        assert!(dest.write_archives("host", "ts", &[ok.clone(), missing]).is_err());
        assert_eq!(fs::read_dir(&host_dir).unwrap().count(), 0, "no partial or final dir left");

        let uri = dest.write_archives("host", "ts", &[ok]).unwrap();
        dest.write_manifest("host", "ts", b"{}").unwrap();
        assert!(Path::new(&uri).join("a.zip").exists());
        assert!(Path::new(&uri).join("manifest.json").exists());
        assert_eq!(fs::read_dir(&host_dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&work);
        let _ = fs::remove_dir_all(&dest.root_path);
    }
}
