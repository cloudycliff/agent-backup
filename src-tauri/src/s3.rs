use aws_credential_types::Credentials;
use aws_sdk_s3::config::{BehaviorVersion, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;

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

fn with_runtime<F, T>(f: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>>,
{
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("创建异步运行时失败: {e}"))?;
    rt.block_on(f)
}

pub fn test_connection(s3: &S3Destination) -> Result<String, String> {
    with_runtime(async {
        let client = build_client(s3)?;
        client
            .head_bucket()
            .bucket(s3.bucket.trim())
            .send()
            .await
            .map_err(|e| format!("连接失败: {e}"))?;
        Ok(format!("连接成功：bucket `{}`", s3.bucket.trim()))
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

        for p in packed.iter().filter(|p| p.status == "ok") {
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
                .map_err(|e| format!("上传 {} 失败: {e}", p.archive_name))?;
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
            .map_err(|e| format!("上传 manifest 失败: {e}"))?;
        Ok(())
    })
}
