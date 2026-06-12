use std::sync::Arc;
use std::path::Path;
use tokio::sync::RwLock;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use axum::{
    Router,
    routing::get,
    extract::{Path as AxumPath, Query, State, Request},
    response::{Response, IntoResponse, Html},
    http::{StatusCode, header},
    body::Body,
};
use zip::write::FileOptions;
use zip::ZipWriter;

#[derive(Clone)]
struct AppState {
    _settings: Arc<RwLock<crate::config::Settings>>,
    zip_semaphore: Arc<tokio::sync::Semaphore>,
}

struct ChannelStream {
    receiver: tokio::sync::mpsc::Receiver<Result<bytes::Bytes, std::io::Error>>,
    _permit: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl futures::Stream for ChannelStream {
    type Item = Result<bytes::Bytes, std::io::Error>;
    
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

pub async fn start_server(settings: Arc<RwLock<crate::config::Settings>>) {
    let bind_addr = {
        let s = settings.read().await;
        s.local_server.bind_addr.clone().unwrap_or_else(|| "0.0.0.0:3000".to_string())
    };

    println!("[Server] Starting embedded web server on {}...", bind_addr);

    let app = Router::new()
        .route("/download/:token", get(handle_download))
        .route("/download/:token/*subpath", get(handle_download))
        .with_state(AppState {
            _settings: settings,
            zip_semaphore: Arc::new(tokio::sync::Semaphore::new(1)), // Max 1 concurrent zip operation to protect STB resources
        });

    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[Server] Failed to bind server to {}: {:?}", bind_addr, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[Server] Web server execution error: {:?}", e);
    }
}

async fn handle_download(
    State(state): State<AppState>,
    AxumPath(params): AxumPath<std::collections::HashMap<String, String>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    request: Request,
) -> impl IntoResponse {
    let token = match params.get("token") {
        Some(t) => t,
        None => return (StatusCode::BAD_REQUEST, "Missing token").into_response(),
    };

    let base_path_str = match crate::db::get_local_download_path(token) {
        Some(p) => p,
        None => return render_error("Download Link Expired", "This download link has expired or is invalid. Please generate a new download link via the bot."),
    };

    let base_path = Path::new(&base_path_str);
    let subpath = params.get("subpath").map(|s| s.as_str()).unwrap_or("");
    let target_path = if subpath.is_empty() {
        base_path.to_path_buf()
    } else {
        let base_name = base_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let subpath_path = Path::new(subpath);
        let mut components = subpath_path.components();
        if let Some(first_comp) = components.next() {
            let first_comp_str = first_comp.as_os_str().to_str().unwrap_or("");
            if first_comp_str == base_name {
                let rest: std::path::PathBuf = components.collect();
                base_path.join(rest)
            } else {
                base_path.join(subpath)
            }
        } else {
            base_path.to_path_buf()
        }
    };

    println!("[Server] Request for token: {}, subpath: {}", token, subpath);

    // Directory traversal security check
    let canonical_base = match base_path.canonicalize() {
        Ok(p) => {
            println!("[Server] canonical_base: {:?}", p);
            p
        }
        Err(e) => {
            eprintln!("[Server] Failed to canonicalize base_path {:?}: {:?}", base_path, e);
            return render_error("Not Found", "The requested download directory was not found on the server.");
        }
    };
    let canonical_target = match target_path.canonicalize() {
        Ok(p) => {
            println!("[Server] canonical_target: {:?}", p);
            p
        }
        Err(e) => {
            eprintln!("[Server] Failed to canonicalize target_path {:?}: {:?}", target_path, e);
            return render_error("Not Found", "The requested file or directory does not exist.");
        }
    };
    if !canonical_target.starts_with(&canonical_base) {
        return (StatusCode::FORBIDDEN, "Access Denied: Path Traversal Detected").into_response();
    }

    if canonical_target.is_dir() {
        let zip_requested = query.get("zip").map(|v| v == "true" || v == "1").unwrap_or(false);
        if zip_requested {
            // Try to acquire the zipping permit to limit concurrent CPU/IO heavy tasks on the STB
            let permit = match state.zip_semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => {
                    return render_error(
                        "Server Busy",
                        "The server is currently zipping another download. Please wait a minute and try again to avoid overloading the system."
                    ).into_response();
                }
            };
            return serve_zipped_directory(&canonical_target, permit).await.into_response();
        } else {
            return serve_directory_index(token, subpath, &canonical_base, &canonical_target).await.into_response();
        }
    } else if canonical_target.is_file() {
        return serve_file(&canonical_target, request.headers()).await.into_response();
    }

    render_error("Not Found", "The requested object type is not supported.")
}

async fn serve_zipped_directory(dir_path: &Path, permit: tokio::sync::OwnedSemaphorePermit) -> Response {
    let temp_dir = std::env::temp_dir();
    let unique_id: u64 = rand::random();
    let temp_file_path = temp_dir.join(format!("magpie_zip_{}.tmp", unique_id));
    
    let dir_path_clone = dir_path.to_path_buf();
    let temp_file_path_clone = temp_file_path.clone();

    // Run zipping inside spawn_blocking
    let zip_res = tokio::task::spawn_blocking(move || {
        let _permit = permit; // Hold permit during CPU-bound zipping
        zip_directory_to_file(&dir_path_clone, &temp_file_path_clone)
    }).await;

    match zip_res {
        Ok(Ok(())) => {
            serve_temp_zip_file(&temp_file_path, dir_path).await
        }
        Ok(Err(e)) => {
            let _ = std::fs::remove_file(&temp_file_path);
            render_error("Zipping Failed", &format!("Unable to zip directory: {:?}", e))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_file_path);
            render_error("Zipping Error", &format!("Spawn blocking task failed: {:?}", e))
        }
    }
}

fn zip_directory_to_file(dir_path: &Path, out_file_path: &Path) -> Result<(), String> {
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
            let name = path.strip_prefix(base_dir).unwrap().to_string_lossy().replace('\\', "/");
            
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

async fn serve_temp_zip_file(file_path: &Path, original_dir: &Path) -> Response {
    let mut file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(e) => return render_error("File Access Error", &format!("Unable to open zipped archive: {:?}", e)),
    };

    let metadata = match file.metadata().await {
        Ok(m) => m,
        Err(e) => return render_error("Metadata Error", &format!("Unable to read zip details: {:?}", e)),
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

    let body = Body::from_stream(ChannelStream { receiver: rx, _permit: None });
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/zip"),
    );
    
    let folder_name = original_dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("archive");
    let content_disposition = format!("attachment; filename=\"{}.zip\"", folder_name);
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&content_disposition).unwrap(),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        header::HeaderValue::from(file_size),
    );

    response
}

async fn serve_directory_index(
    token: &str,
    subpath: &str,
    base_dir: &Path,
    current_dir: &Path,
) -> Response {
    let mut files_list = Vec::new();
    
    // Check if we can display a back link
    let is_root = current_dir == base_dir;
    let parent_link = if !is_root {
        let rel_parent = current_dir.parent()
            .and_then(|p| p.strip_prefix(base_dir).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if rel_parent.is_empty() {
            Some(format!("/download/{}", token))
        } else {
            Some(format!("/download/{}/{}", token, rel_parent))
        }
    } else {
        None
    };

    let entries = match std::fs::read_dir(current_dir) {
        Ok(e) => e,
        Err(err) => return render_error("Read Directory Error", &format!("Failed to read directory: {:?}", err)),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let is_dir = path.is_dir();
        
        let size_str = if is_dir {
            "-".to_string()
        } else {
            let metadata = std::fs::metadata(&path);
            let bytes = metadata.map(|m| m.len()).unwrap_or(0);
            format_size(bytes)
        };

        let link = if subpath.is_empty() {
            format!("/download/{}/{}", token, name)
        } else {
            format!("/download/{}/{}/{}", token, subpath.trim_end_matches('/'), name)
        };

        files_list.push(format!(
            r#"
            <div class="file-item">
                <div class="file-info">
                    <span class="file-icon">{}</span>
                    <a href="{}" class="file-name">{}</a>
                </div>
                <span class="file-size">{}</span>
            </div>
            "#,
            if is_dir { "📁" } else { "📄" },
            link,
            name,
            size_str
        ));
    }

    let folder_name = current_dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Downloads");

    let zip_link = if subpath.is_empty() {
        format!("/download/{}?zip=true", token)
    } else {
        format!("/download/{}/{}?zip=true", token, subpath.trim_end_matches('/'))
    };

    // Fetch expiry info
    let conn = crate::db::get_connection().ok();
    let expires_at = conn.and_then(|c| {
        let mut stmt = c.prepare("SELECT expires_at FROM local_downloads WHERE token = ?").ok()?;
        stmt.query_row(rusqlite::params![token], |r| r.get::<_, i64>(0)).ok()
    }).unwrap_or(0);

    let html_content = format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Download - {folder_name}</title>
    <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;600;700&display=swap" rel="stylesheet">
    <style>
        :root {{
            --bg-color: #0c0d14;
            --text-color: #f1f2f6;
            --glass-bg: rgba(255, 255, 255, 0.03);
            --glass-border: rgba(255, 255, 255, 0.08);
            --glass-hover: rgba(255, 255, 255, 0.07);
            --primary: #4f46e5;
            --primary-hover: #4338ca;
            --accent: #10b981;
        }}
        
        * {{
            box-sizing: border-box;
            margin: 0;
            padding: 0;
        }}
        
        body {{
            font-family: 'Outfit', sans-serif;
            background-color: var(--bg-color);
            background-image: 
                radial-gradient(circle at 10% 20%, rgba(79, 70, 229, 0.08) 0%, transparent 40%),
                radial-gradient(circle at 90% 80%, rgba(16, 185, 129, 0.05) 0%, transparent 40%);
            color: var(--text-color);
            min-height: 100vh;
            display: flex;
            justify-content: center;
            align-items: center;
            padding: 20px;
        }}
        
        .container {{
            width: 100%;
            max-width: 800px;
            background: var(--glass-bg);
            backdrop-filter: blur(20px);
            -webkit-backdrop-filter: blur(20px);
            border: 1px solid var(--glass-border);
            border-radius: 24px;
            padding: 40px;
            box-shadow: 0 20px 50px rgba(0, 0, 0, 0.3);
            animation: fadeIn 0.6s ease-out;
        }}
        
        @keyframes fadeIn {{
            from {{ opacity: 0; transform: translateY(15px); }}
            to {{ opacity: 1; transform: translateY(0); }}
        }}
        
        .header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 30px;
            border-bottom: 1px solid var(--glass-border);
            padding-bottom: 20px;
            flex-wrap: wrap;
            gap: 20px;
        }}
        
        .title-area {{
            flex: 1;
            min-width: 250px;
        }}
        
        .folder-title {{
            font-size: 24px;
            font-weight: 700;
            letter-spacing: -0.5px;
            color: #ffffff;
            margin-bottom: 5px;
            word-break: break-all;
        }}
        
        .timer-badge {{
            display: inline-flex;
            align-items: center;
            background: rgba(239, 68, 68, 0.1);
            color: #ef4444;
            border: 1px solid rgba(239, 68, 68, 0.2);
            padding: 4px 12px;
            border-radius: 50px;
            font-size: 13px;
            font-weight: 600;
        }}
        
        .actions-area {{
            display: flex;
            gap: 12px;
        }}
        
        .btn {{
            display: inline-flex;
            align-items: center;
            justify-content: center;
            padding: 12px 20px;
            border-radius: 12px;
            font-weight: 600;
            text-decoration: none;
            cursor: pointer;
            transition: all 0.2s ease;
            font-size: 14px;
        }}
        
        .btn-primary {{
            background: var(--primary);
            color: #ffffff;
            border: none;
            box-shadow: 0 4px 14px rgba(79, 70, 229, 0.3);
        }}
        
        .btn-primary:hover {{
            background: var(--primary-hover);
            transform: translateY(-2px);
            box-shadow: 0 6px 20px rgba(79, 70, 229, 0.4);
        }}
        
        .btn-secondary {{
            background: var(--glass-bg);
            color: var(--text-color);
            border: 1px solid var(--glass-border);
        }}
        
        .btn-secondary:hover {{
            background: var(--glass-hover);
            transform: translateY(-2px);
        }}
        
        .files-list {{
            display: flex;
            flex-direction: column;
            gap: 8px;
            margin-bottom: 20px;
            max-height: 500px;
            overflow-y: auto;
            padding-right: 5px;
        }}
        
        .files-list::-webkit-scrollbar {{
            width: 6px;
        }}
        
        .files-list::-webkit-scrollbar-track {{
            background: transparent;
        }}
        
        .files-list::-webkit-scrollbar-thumb {{
            background: var(--glass-border);
            border-radius: 10px;
        }}
        
        .file-item {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 14px 20px;
            background: var(--glass-bg);
            border: 1px solid var(--glass-border);
            border-radius: 14px;
            transition: all 0.2s ease;
        }}
        
        .file-item:hover {{
            background: var(--glass-hover);
            border-color: rgba(255, 255, 255, 0.15);
            transform: scale(1.005);
        }}
        
        .file-info {{
            display: flex;
            align-items: center;
            gap: 15px;
            flex: 1;
            min-width: 0;
        }}
        
        .file-icon {{
            font-size: 20px;
            user-select: none;
        }}
        
        .file-name {{
            color: var(--text-color);
            text-decoration: none;
            font-weight: 500;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
            transition: color 0.2s ease;
        }}
        
        .file-name:hover {{
            color: #ffffff;
        }}
        
        .file-size {{
            font-size: 14px;
            color: rgba(255, 255, 255, 0.5);
            font-weight: 500;
            margin-left: 15px;
        }}
        
        .footer {{
            text-align: center;
            margin-top: 30px;
            font-size: 13px;
            color: rgba(255, 255, 255, 0.3);
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <div class="title-area">
                <h1 class="folder-title">{folder_name}</h1>
                <div class="timer-badge" id="countdown-badge">🕒 Loading...</div>
            </div>
            <div class="actions-area">
                {parent_link}
                <a href="{zip_link}" class="btn btn-primary">⚡ Download Entire Folder (.zip)</a>
            </div>
        </div>
        
        <div class="files-list">
            {files_list}
        </div>
        
        <div class="footer">
            Powered by Magpie Server • Secure Download Link
        </div>
    </div>

    <script>
        const expiresAt = {expires_at};
        
        function updateCountdown() {{
            const now = Math.floor(Date.now() / 1000);
            const remaining = expiresAt - now;
            const badge = document.getElementById('countdown-badge');
            
            if (remaining <= 0) {{
                badge.innerText = "🚨 Expired";
                badge.style.background = "rgba(239, 68, 68, 0.2)";
                badge.style.color = "#ef4444";
                badge.style.borderColor = "rgba(239, 68, 68, 0.4)";
                setTimeout(() => location.reload(), 2000);
                return;
            }}
            
            const hours = Math.floor(remaining / 3600);
            const minutes = Math.floor((remaining % 3600) / 60);
            const seconds = remaining % 60;
            
            let timeStr = "";
            if (hours > 0) timeStr += hours + "h ";
            if (minutes > 0 || hours > 0) timeStr += minutes + "m ";
            timeStr += seconds + "s";
            
            badge.innerText = "🕒 Link expires in " + timeStr;
        }}
        
        if (expiresAt > 0) {{
            updateCountdown();
            setInterval(updateCountdown, 1000);
        }} else {{
            document.getElementById('countdown-badge').style.display = 'none';
        }}
    </script>
</body>
</html>"##,
        folder_name = folder_name,
        parent_link = parent_link.map(|l| format!(r#"<a href="{}" class="btn btn-secondary">⬅️ Back</a>"#, l)).unwrap_or_default(),
        zip_link = zip_link,
        files_list = files_list.join("\n"),
        expires_at = expires_at
    );

    Html(html_content).into_response()
}

async fn serve_file(file_path: &Path, headers: &header::HeaderMap) -> Response {
    let mut file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(e) => return render_error("File Access Error", &format!("Unable to open file: {:?}", e)),
    };

    let metadata = match file.metadata().await {
        Ok(m) => m,
        Err(e) => return render_error("Metadata Error", &format!("Unable to read file details: {:?}", e)),
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
        return render_error("File Seek Error", &format!("Unable to process file stream: {:?}", e));
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

    let body = Body::from_stream(ChannelStream { receiver: rx, _permit: None });
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
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(mime),
    );

    let filename = file_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    
    let content_disposition = format!("inline; filename*=UTF-8''{}", crate::utils::percent_encode(filename));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&content_disposition).unwrap(),
    );

    response
}

fn parse_range(range_header: &str, file_size: u64) -> Option<(u64, u64)> {
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

fn guess_mime(path: &Path) -> &'static str {
    let ext = path.extension()
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

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn render_error(title: &str, message: &str) -> Response {
    let html_content = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Error - {}</title>
    <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;600;700&display=swap" rel="stylesheet">
    <style>
        :root {{
            --bg-color: #0c0d14;
            --text-color: #f1f2f6;
            --glass-bg: rgba(255, 255, 255, 0.03);
            --glass-border: rgba(255, 255, 255, 0.08);
            --primary: #ef4444;
        }}
        body {{
            font-family: 'Outfit', sans-serif;
            background-color: var(--bg-color);
            color: var(--text-color);
            min-height: 100vh;
            display: flex;
            justify-content: center;
            align-items: center;
            padding: 20px;
        }}
        .container {{
            width: 100%;
            max-width: 500px;
            background: var(--glass-bg);
            backdrop-filter: blur(20px);
            border: 1px solid var(--glass-border);
            border-radius: 24px;
            padding: 40px;
            text-align: center;
            box-shadow: 0 20px 50px rgba(0, 0, 0, 0.3);
        }}
        .icon {{
            font-size: 50px;
            margin-bottom: 20px;
        }}
        h1 {{
            font-size: 24px;
            margin-bottom: 15px;
            color: #ffffff;
        }}
        p {{
            font-size: 15px;
            color: rgba(255, 255, 255, 0.6);
            line-height: 1.6;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="icon">⚠️</div>
        <h1>{}</h1>
        <p>{}</p>
    </div>
</body>
</html>"#,
        title, title, message
    );
    Html(html_content).into_response()
}
