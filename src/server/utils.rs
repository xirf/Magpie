use axum::response::{Html, IntoResponse, Response};

pub fn format_size(bytes: u64) -> String {
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

pub fn render_error(title: &str, message: &str) -> Response {
    let html_content = format!(
        r#""<!DOCTYPE html>
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
        <div class="icon">??</div>
        <h1>{}</h1>
        <p>{}</p>
    </div>
</body>
</html>""#,
        title, title, message
    );
    Html(html_content).into_response()
}