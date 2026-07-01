use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::ChannelStream;
use super::utils::render_error;

pub async fn serve_file(file_path: &Path, headers: &header::HeaderMap) -> Response {
    let mut file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(e) => {
            return render_error(
                "File Access Error",
                &format!("Unable to open file: {:?}", e),
            )
        }
    };

    let metadata = match file.metadata().await {
        Ok(m) => m,
        Err(e) => {
            return render_error(
                "Metadata Error",
                &format!("Unable to read file details: {:?}", e),
            )
        }
    };
    let file_size = metadata.len();

    let mut range = None;
    if let Some(range_val) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        range = parse_range(range_val, file_size);
    }

    let (start, end, status) = match range {
        Some((s, e)) => (s, e, StatusCode::PARTIAL_CONTENT),
        None => (0, file_size - 1, StatusCode::OK),
    };

    let content_length = end - start + 1;
    if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
        return render_error(
            "File Seek Error",
            &format!("Unable to process file stream: {:?}", e),
        );
    }

    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        let mut remaining = content_length;
        let mut buffer = vec![0u8; 65536];
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
    });

    let body = Body::from_stream(ChannelStream {
        receiver: rx,
        _permit: None,
    });
    let mut response = Response::new(body);
    *response.status_mut() = status;

    if status == StatusCode::PARTIAL_CONTENT {
        let content_range = format!("bytes {}-{}/{}", start, end, file_size);
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            header::HeaderValue::from_str(&content_range).unwrap(),
        );
    }

    response.headers_mut().insert(
        header::ACCEPT_RANGES,
        header::HeaderValue::from_static("bytes"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        header::HeaderValue::from(content_length),
    );

    let mime = guess_mime(file_path);
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, header::HeaderValue::from_static(mime));

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    let content_disposition = format!(
        "inline; filename*=UTF-8''{}",
        crate::utils::percent_encode(filename)
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&content_disposition).unwrap(),
    );

    response
}

pub fn parse_range(range_header: &str, file_size: u64) -> Option<(u64, u64)> {
    if !range_header.starts_with("bytes=") {
        return None;
    }
    let range = range_header.trim_start_matches("bytes=");
    let parts: Vec<&str> = range.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    let start = parts[0].parse::<u64>().ok()?;
    let end = if parts[1].is_empty() {
        file_size - 1
    } else {
        parts[1].parse::<u64>().ok()?
    };
    if start <= end && end < file_size {
        Some((start, end))
    } else {
        None
    }
}

pub fn guess_mime(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "txt" => "text/plain; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}