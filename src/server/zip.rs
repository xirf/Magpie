use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use zip::write::FileOptions;
use zip::ZipWriter;

use super::ChannelStream;
use super::utils::render_error;

pub async fn serve_zipped_directory(
    dir_path: &Path,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Response {
    let temp_dir = std::env::temp_dir();
    let unique_id: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_else(|_| {
            let p = &dir_path as *const _ as usize as u64;
            p
        });
    let temp_file_path = temp_dir.join(format!("magpie_zip_{}.tmp", unique_id));

    let dir_path_clone = dir_path.to_path_buf();
    let temp_file_path_clone = temp_file_path.clone();

    // Run zipping inside spawn_blocking
    let zip_res = tokio::task::spawn_blocking(move || {
        let _permit = permit; // Hold permit during CPU-bound zipping
        zip_directory_to_file(&dir_path_clone, &temp_file_path_clone)
    })
    .await;

    match zip_res {
        Ok(Ok(())) => serve_temp_zip_file(&temp_file_path, dir_path).await,
        Ok(Err(e)) => {
            let _ = std::fs::remove_file(&temp_file_path);
            render_error(
                "Zipping Failed",
                &format!("Unable to zip directory: {:?}", e),
            )
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_file_path);
            render_error(
                "Zipping Error",
                &format!("Spawn blocking task failed: {:?}", e),
            )
        }
    }
}

pub fn zip_directory_to_file(dir_path: &Path, out_file_path: &Path) -> Result<(), String> {
    let file = std::fs::File::create(out_file_path)
        .map_err(|e| format!("Failed to create output zip file: {:?}", e))?;
    let mut zip = ZipWriter::new(file);

    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);

    fn walk_zip(
        dir: &Path,
        base_dir: &Path,
        zip: &mut ZipWriter<std::fs::File>,
        options: FileOptions,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = path
                .strip_prefix(base_dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");

            if path.is_dir() {
                zip.add_directory(&name, options)?;
                walk_zip(&path, base_dir, zip, options)?;
            } else if path.is_file() {
                zip.start_file(&name, options)?;
                let mut file = std::fs::File::open(&path)?;
                std::io::copy(&mut file, zip)?;
            }
        }
        Ok(())
    }

    let base_dir = dir_path.parent().unwrap_or(dir_path);
    walk_zip(dir_path, base_dir, &mut zip, options)
        .map_err(|e| format!("Zipping failed: {:?}", e))?;

    zip.finish()
        .map_err(|e| format!("Failed to finish zip writer: {:?}", e))?;
    Ok(())
}

pub async fn serve_temp_zip_file(file_path: &Path, original_dir: &Path) -> Response {
    let mut file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(e) => {
            return render_error(
                "File Access Error",
                &format!("Unable to open zipped archive: {:?}", e),
            )
        }
    };

    let metadata = match file.metadata().await {
        Ok(m) => m,
        Err(e) => {
            return render_error(
                "Metadata Error",
                &format!("Unable to read zip details: {:?}", e),
            )
        }
    };
    let file_size = metadata.len();

    let (tx, rx) = tokio::sync::mpsc::channel(16);
    let file_path_clone = file_path.to_path_buf();

    tokio::spawn(async move {
        let mut buffer = vec![0u8; 65536];
        let mut remaining = file_size;
        while remaining > 0 {
            let to_read = std::cmp::min(remaining, buffer.len() as u64) as usize;
            match file.read_exact(&mut buffer[..to_read]).await {
                Ok(_) => {
                    let bytes = bytes::Bytes::copy_from_slice(&buffer[..to_read]);
                    if tx.send(Ok(bytes)).await.is_err() {
                        break;
                    }
                    remaining -= to_read as u64;
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            }
        }
        // Cleanup temp file
        let _ = tokio::fs::remove_file(&file_path_clone).await;
    });

    let body = Body::from_stream(ChannelStream {
        receiver: rx,
        _permit: None,
    });
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/zip"),
    );

    let folder_name = original_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("archive");
    let content_disposition = format!("attachment; filename=\"{}.zip\"", folder_name);
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&content_disposition).unwrap(),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, header::HeaderValue::from(file_size));

    response
}