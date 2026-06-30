use crate::config::Settings;
use crate::db::{cache_presigned_url, get_cached_presigned_url};
use rand::Rng;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::Region;
use std::path::Path;

pub fn get_s3_bucket(settings: &Settings, use_public: bool) -> Result<Box<Bucket>, String> {
    let s3_config = &settings.s3;
    if !s3_config.enabled {
        return Err("S3 is not enabled.".to_string());
    }

    let access_key = s3_config
        .access_key
        .as_deref()
        .ok_or_else(|| "S3 access key is missing.".to_string())?;
    let secret_key = s3_config
        .secret_key
        .as_deref()
        .ok_or_else(|| "S3 secret key is missing.".to_string())?;
    let bucket_name = s3_config
        .bucket
        .as_deref()
        .ok_or_else(|| "S3 bucket name is missing.".to_string())?;

    let credentials = Credentials::new(Some(access_key), Some(secret_key), None, None, None)
        .map_err(|e| e.to_string())?;

    let region_name = s3_config
        .region
        .clone()
        .unwrap_or_else(|| "us-east-1".to_string());
    let endpoint_opt = if use_public {
        s3_config
            .public_url
            .clone()
            .or_else(|| s3_config.endpoint.clone())
    } else {
        s3_config.endpoint.clone()
    };

    let region = if let Some(ref endpoint) = endpoint_opt {
        Region::Custom {
            region: region_name,
            endpoint: endpoint.clone(),
        }
    } else {
        region_name
            .parse()
            .map_err(|e| format!("Invalid region: {:?}", e))?
    };

    let mut bucket = Bucket::new(bucket_name, region, credentials).map_err(|e| e.to_string())?;

    if endpoint_opt.is_some() {
        bucket = bucket.with_path_style();
    }

    Ok(bucket)
}

pub async fn generate_presigned_url(settings: &Settings, key: &str) -> Result<String, String> {
    if let Some(cached) = get_cached_presigned_url(key) {
        return Ok(cached);
    }

    let bucket = get_s3_bucket(settings, true)?;
    let expiry = settings.s3.link_expiry;

    let url = bucket
        .presign_get(key, expiry as u32, None)
        .await
        .map_err(|e| e.to_string())?;

    let _ = cache_presigned_url(key, &url, expiry as i64);
    Ok(url)
}

pub async fn upload_to_s3(settings: &Settings, local_path: &Path, key: &str) -> Result<(), String> {
    let bucket = get_s3_bucket(settings, false)?;
    let bytes = std::fs::read(local_path).map_err(|e| format!("Failed to read file: {:?}", e))?;

    bucket
        .put_object(key, &bytes)
        .await
        .map_err(|e| format!("S3 PutObject error: {:?}", e))?;

    Ok(())
}

pub async fn upload_folder_or_file_to_s3(
    settings: &Settings,
    local_path: &Path,
    key_prefix: &str,
) -> Result<(), String> {
    if !local_path.exists() {
        return Err(format!("Local path does not exist: {:?}", local_path));
    }

    if local_path.is_file() {
        let filename = local_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        let s3_key = if key_prefix.is_empty() {
            filename.to_string()
        } else {
            format!("{}/{}", key_prefix, filename)
        };
        upload_to_s3(settings, local_path, &s3_key).await?;
    } else if local_path.is_dir() {
        let mut files = Vec::new();
        fn get_files_recursively(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        get_files_recursively(&path, files);
                    } else {
                        files.push(path);
                    }
                }
            }
        }
        get_files_recursively(local_path, &mut files);

        let base_parent = local_path.parent().unwrap_or(local_path);
        for file_path in files {
            if let Ok(rel_path) = file_path.strip_prefix(base_parent) {
                let rel_str = rel_path.to_string_lossy().replace('\\', "/");
                let s3_key = if key_prefix.is_empty() {
                    rel_str
                } else {
                    format!("{}/{}", key_prefix, rel_str)
                };
                upload_to_s3(settings, &file_path, &s3_key).await?;
            }
        }
    }
    Ok(())
}

pub async fn get_download_link(
    settings: &Settings,
    torrent: &crate::torrent_client::Torrent,
) -> Result<Option<String>, String> {
    let local_enabled = settings.local_server.enabled;
    let s3_enabled = settings.s3.enabled;

    if !local_enabled && !s3_enabled {
        return Ok(None);
    }

    let content_path_str = torrent.content_path.clone().unwrap_or_default();
    if content_path_str.is_empty() {
        return Err("Torrent content path is missing.".to_string());
    }

    if local_enabled {
        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let expires_at = now + settings.local_server.link_expiry as i64;

        crate::db::cache_local_download(&token, &content_path_str, expires_at)
            .map_err(|e| format!("Failed to cache local download link: {:?}", e))?;

        let base_url = settings
            .local_server
            .base_url
            .as_deref()
            .unwrap_or("http://localhost/");
        let base_url_trimmed = base_url.trim_end_matches('/');
        let torrent_name_encoded = crate::utils::percent_encode(&torrent.name);
        let url = format!(
            "{}/download/{}/{}",
            base_url_trimmed, token, torrent_name_encoded
        );
        return Ok(Some(url));
    }

    let content_path = std::path::Path::new(&content_path_str);
    let mut key = torrent.name.clone();
    if content_path.exists() {
        if content_path.is_dir() {
            let mut files = Vec::new();
            fn get_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            get_files(&path, files);
                        } else {
                            files.push(path);
                        }
                    }
                }
            }
            get_files(content_path, &mut files);
            if !files.is_empty() {
                files.sort_by_key(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0));
                if let Some(largest) = files.last() {
                    let base_parent = content_path.parent().unwrap_or(content_path);
                    if let Ok(rel) = largest.strip_prefix(base_parent) {
                        key = rel.to_string_lossy().replace('\\', "/");
                    }
                }
            }
        } else {
            key = content_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&torrent.name)
                .to_string();
        }
    }

    let url = generate_presigned_url(settings, &key).await?;
    Ok(Some(url))
}
