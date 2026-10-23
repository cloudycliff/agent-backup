use aws_credential_types::Credentials;
use aws_sdk_s3::config::{BehaviorVersion, Region};
use aws_sdk_s3::error::DisplayErrorContext;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;

use crate::backup::Status;
use crate::config::S3Destination;
use crate::packager::PackedSource;

fn join_key(prefix: &str, parts: &[&str]) -> String {
    let mut segs: Vec<String> = Vec::new();
    let trimmed = prefix.trim().trim_matches('/');
    if !trimmed.is_empty() {
        segs.push(trimmed.to_string());
    }
    for p in parts {
        let t = p.trim().trim_matches('/');
        if !t.is_empty() {
            segs.push(t.to_string());
        }
    }
    segs.join("/")
}

fn build_client(s3: &S3Destination) -> Result<Client, String> {
    if s3.endpoint.trim().is_empty() {
        return Err("S3 endpoint 不能为空".into());
    }
    if s3.bucket.trim().is_empty() {
        return Err("S3 bucket 不能为空".into());
    }
    if s3.access_key.trim().is_empty() || s3.secret_key.trim().is_empty() {
        return Err("S3 Access Key / Secret Key 不能为空".into());
    }
    let region = if s3.region.trim().is_empty() {
        "auto".to_string()
    } else {
        s3.region.trim().to_string()
    };
    let creds = Credentials::new(
        s3.access_key.trim(),
        s3.secret_key.trim(),
        None,
        None,
        "agent-backup",
    );
    let conf = aws_sdk_s3::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new(region))
        .credentials_provider(creds)
        .endpoint_url(s3.endpoint.trim())
        .force_path_style(true)
        .build();
    Ok(Client::from_conf(conf))
}

/// Runs on Tauri's shared runtime. Callers must be on a plain or `spawn_blocking`
/// thread, never inside an async task.
fn with_runtime<F, T>(f: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    tauri::async_runtime::block_on(f)
}

fn describe<E: std::error::Error>(e: E) -> String {
    DisplayErrorContext(e).to_string()
}

/// Uploads a small object under the prefix, which is exactly what a backup needs,
/// so keys limited to PutObject pass. Deleting it afterwards is best effort.
pub fn test_connection(s3: &S3Destination) -> Result<String, String> {
    with_runtime(async {
        let client = build_client(s3)?;
        let bucket = s3.bucket.trim();
        let key = join_key(&s3.prefix, &[".agent-backup-write-test"]);
        client
            .put_object()
            .bucket(bucket)
            .key(&key)
            .body(ByteStream::from_static(b"agent-backup write test"))
            .send()
            .await
            .map_err(|e| format!("写入测试失败: {}", describe(e)))?;
        let mut msg = format!("写入测试成功：s3://{bucket}/{key}");
        if client.delete_object().bucket(bucket).key(&key).send().await.is_err() {
            msg.push_str("（测试文件未能删除，可手动删除；只有上传权限也能正常备份）");
        }
        Ok(msg)
    })
}

pub fn upload_archives(
    s3: &S3Destination,
    hostname: &str,
    timestamp: &str,
    packed: &[PackedSource],
) -> Result<String, String> {
    with_runtime(async {
        let client = build_client(s3)?;
        let bucket = s3.bucket.trim();
        let base = join_key(&s3.prefix, &["backups", hostname, timestamp]);

        for p in packed.iter().filter(|p| p.status == Status::Ok) {
            let key = format!("{}/{}", base, p.archive_name);
            let body = ByteStream::from_path(&p.archive_path)
                .await
                .map_err(|e| format!("读取 {} 失败: {e}", p.archive_name))?;
            client
                .put_object()
                .bucket(bucket)
                .key(&key)
                .body(body)
                .content_type("application/zip")
                .send()
                .await
                .map_err(|e| format!("上传 {} 失败: {}", p.archive_name, describe(e)))?;
        }

        Ok(format!("s3://{bucket}/{base}"))
    })
}

pub fn upload_manifest(
    s3: &S3Destination,
    hostname: &str,
    timestamp: &str,
    manifest_bytes: &[u8],
) -> Result<(), String> {
    with_runtime(async {
        let client = build_client(s3)?;
        let bucket = s3.bucket.trim();
        let key = join_key(
            &s3.prefix,
            &["backups", hostname, timestamp, "manifest.json"],
        );
        client
            .put_object()
            .bucket(bucket)
            .key(&key)
            .body(ByteStream::from(manifest_bytes.to_vec()))
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| format!("上传 manifest 失败: {}", describe(e)))?;
        Ok(())
    })
}
