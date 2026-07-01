use axum::response::{Html, IntoResponse, Response};
use std::path::Path;

use super::utils::{format_size, render_error};

pub async fn serve_directory_index(
    token: &str,
    subpath: &str,
    base_dir: &Path,
    current_dir: &Path,
) -> Response {
    let mut files_list = Vec::new();

    // Check if we can display a back link
    let is_root = current_dir == base_dir;
    let parent_link = if !is_root {
        let rel_parent = current_dir
            .parent()
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
        Err(err) => {
            return render_error(
                "Read Directory Error",
                &format!("Failed to read directory: {:?}", err),
            )
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
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
            format!(
                "/download/{}/{}/{}",
                token,
                subpath.trim_end_matches('/'),
                name
            )
        };

        files_list.push(format!(
            r#""
            <div class="file-item">
                <div class="file-info">
                    <span class="file-icon">{}</span>
                    <a href="{}" class="file-name">{}</a>
                </div>
                <span class="file-size">{}</span>
            </div>
            ""#,
            if is_dir { "??" } else { "??" },
            link,
            name,
            size_str
        ));
    }

    let folder_name = current_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Downloads");

    let zip_link = if subpath.is_empty() {
        format!("/download/{}?zip=true", token)
    } else {
        format!(
            "/download/{}/{}?zip=true",
            token,
            subpath.trim_end_matches('/')
        )
    };

    // Fetch expiry info
    let conn = crate::db::get_connection().ok();
    let expires_at = conn
        .and_then(|c| {
            let mut stmt = c
                .prepare("SELECT expires_at FROM local_downloads WHERE token = ?")
                .ok()?;
            stmt.query_row(rusqlite::params![token], |r| r.get::<_, i64>(0))
                .ok()
        })
        .unwrap_or(0);

    let html_content = format!(
        r###""<!DOCTYPE html>
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
                <div class="timer-badge" id="countdown-badge">?? Loading...</div>
            </div>
            <div class="actions-area">
                {parent_link}
                <a href="{zip_link}" class="btn btn-primary">? Download Entire Folder (.zip)</a>
            </div>
        </div>
        
        <div class="files-list">
            {files_list}
        </div>
        
        <div class="footer">
            Powered by Magpie Server &bull; Secure Download Link
        </div>
    </div>

    <script>
        const expiresAt = {expires_at};
        
        function updateCountdown() {{
            const now = Math.floor(Date.now() / 1000);
            const remaining = expiresAt - now;
            const badge = document.getElementById('countdown-badge');
            
            if (remaining <= 0) {{
                badge.innerText = "?? Expired";
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
            
            badge.innerText = "?? Link expires in " + timeStr;
        }}
        
        if (expiresAt > 0) {{
            updateCountdown();
            setInterval(updateCountdown, 1000);
        }} else {{
            document.getElementById('countdown-badge').style.display = 'none';
        }}
    </script>
</body>
</html>""###,
        folder_name = folder_name,
        parent_link = parent_link
            .map(|l| format!(r#""<a href="{}" class="btn btn-secondary">?? Back</a>""#, l))
            .unwrap_or_default(),
        zip_link = zip_link,
        files_list = files_list.join("\n"),
        expires_at = expires_at
    );

    Html(html_content).into_response()
}