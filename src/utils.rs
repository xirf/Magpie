pub fn read_cpu_temp() -> String {
    std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .map(|t| format!("{}", t / 1000))
        .unwrap_or_else(|| "N/A".to_string())
}

pub fn extract_hash_from_magnet(magnet: &str) -> Option<String> {
    let lower = magnet.to_lowercase();
    if let Some(pos) = lower.find("xt=urn:btih:") {
        let start = pos + "xt=urn:btih:".len();
        let rest = &magnet[start..];
        let end = rest.find('&').unwrap_or(rest.len());
        Some(rest[..end].to_string())
    } else {
        None
    }
}

fn starts_parameter(part: &str) -> bool {
    if part.starts_with('&') || part.starts_with('?') {
        return true;
    }
    let keys = ["xt=", "dn=", "tr=", "xl=", "kt=", "ws=", "as=", "xs="];
    for key in keys {
        if part.starts_with(key) {
            return true;
        }
    }
    false
}

fn is_dn_active(magnet_link: &str) -> bool {
    let mut last_key = "";
    if let Some(last_eq) = magnet_link.rfind('=') {
        let prefix = &magnet_link[..last_eq];
        if let Some(last_amp) = prefix.rfind('&').or_else(|| prefix.rfind('?')) {
            last_key = &prefix[last_amp + 1..];
        }
    }
    last_key == "dn"
}

pub fn extract_clean_magnet(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if let Some(start_idx) = lower.find("magnet:?") {
        let rest = &text[start_idx..];
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        let mut magnet_link = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                magnet_link.push_str(part);
            } else {
                let is_parameter_start = starts_parameter(part);
                let is_tracker_url = part.starts_with("http://")
                    || part.starts_with("https://")
                    || part.starts_with("udp://")
                    || part.starts_with("wss://");

                if is_parameter_start || is_tracker_url {
                    if !part.starts_with('&')
                        && !magnet_link.ends_with('&')
                        && !magnet_link.ends_with('?')
                    {
                        magnet_link.push('&');
                    }
                    magnet_link.push_str(part);
                } else if is_dn_active(&magnet_link) {
                    if !magnet_link.ends_with('%') && !part.starts_with('%') {
                        magnet_link.push_str("%20");
                    }
                    magnet_link.push_str(part);
                } else {
                    break;
                }
            }
        }

        if magnet_link.to_lowercase().contains("xt=urn:") {
            return Some(magnet_link);
        }
    }
    None
}

pub fn convert_size(size_bytes: u64) -> String {
    if size_bytes == 0 {
        return "0 B".to_string();
    }
    let size_names = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let i = ((size_bytes as f64).ln() / (1024.0_f64).ln()).floor() as usize;
    let i = i.min(size_names.len() - 1);
    let p = 1024.0_f64.powi(i as i32);
    let s = ((size_bytes as f64 / p) * 100.0).round() / 100.0;
    format!("{} {}", s, size_names[i])
}

pub fn convert_eta(seconds: i64) -> String {
    if seconds == 8640000 || seconds < 0 {
        return "∞".to_string();
    }
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    let time_str = format!("{:02}:{:02}:{:02}", hours, minutes, secs);
    if days > 0 {
        format!(
            "{} day{}, {}",
            days,
            if days > 1 { "s" } else { "" },
            time_str
        )
    } else {
        time_str
    }
}

pub fn format_progress(progress: f64, width: usize) -> String {
    let progress = progress.max(0.0).min(1.0);
    let filled = (progress * width as f64).floor() as usize;
    let bar = "█".repeat(filled) + &"░".repeat(width - filled);
    let percent = (progress * 100.0).floor() as i32;
    format!("{: >3}%|{}|\n", percent, bar)
}

pub fn escape_markdown(text: &str) -> String {
    let mut escaped = String::new();
    for c in text.chars() {
        if "_*[]()~`>#+-=|{}.!".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

pub fn percent_encode(input: &str) -> String {
    let mut encoded = String::new();
    for b in input.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(*b as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percent_encode() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
        assert_eq!(percent_encode("a/b/c.txt"), "a/b/c.txt");
        assert_eq!(percent_encode("Utada Hikaru - First Love (1999) [FLAC]/4. First Love (John Luongo Remix).flac"), "Utada%20Hikaru%20-%20First%20Love%20%281999%29%20%5BFLAC%5D/4.%20First%20Love%20%28John%20Luongo%20Remix%29.flac");
    }

    #[test]
    fn test_convert_size() {
        assert_eq!(convert_size(0), "0 B");
        assert_eq!(convert_size(1024), "1 KB");
        assert_eq!(convert_size(1536), "1.5 KB");
        assert_eq!(convert_size(1048576), "1 MB");
        assert_eq!(convert_size(1073741824), "1 GB");
    }

    #[test]
    fn test_convert_eta() {
        assert_eq!(convert_eta(-1), "∞");
        assert_eq!(convert_eta(8640000), "∞");
        assert_eq!(convert_eta(0), "00:00:00");
        assert_eq!(convert_eta(45), "00:00:45");
        assert_eq!(convert_eta(3665), "01:01:05");
        assert_eq!(convert_eta(90065), "1 day, 01:01:05");
        assert_eq!(convert_eta(176465), "2 days, 01:01:05");
    }

    #[test]
    fn test_format_progress() {
        assert_eq!(format_progress(0.0, 10), "  0%|░░░░░░░░░░|\n");
        assert_eq!(format_progress(0.5, 10), " 50%|█████░░░░░|\n");
        assert_eq!(format_progress(1.0, 10), "100%|██████████|\n");
        assert_eq!(format_progress(1.5, 10), "100%|██████████|\n");
        assert_eq!(format_progress(-0.5, 10), "  0%|░░░░░░░░░░|\n");
    }

    #[test]
    fn test_escape_markdown() {
        assert_eq!(escape_markdown("hello-world"), "hello\\-world");
        assert_eq!(escape_markdown("user_name"), "user\\_name");
        assert_eq!(escape_markdown("*bold*"), "\\*bold\\*");
        assert_eq!(escape_markdown("[link]"), "\\[link\\]");
        assert_eq!(escape_markdown("text."), "text\\.");
    }

    #[test]
    fn test_extract_clean_magnet() {
        let m1 = "magnet:?xt=urn:btih:feb1f53aa5b3a1660d853c158710175f90d20074&dn=test";
        assert_eq!(extract_clean_magnet(m1), Some(m1.to_string()));

        let m2 = "magnet:?\nxt=urn:btih:feb1f53aa5b3a1660d853c158710175f90d20074&dn=test";
        assert_eq!(extract_clean_magnet(m2), Some(m1.to_string()));

        let m3 =
            "magnet:?xt=urn:btih:feb1f53aa5b3a1660d853c158710175f90d20074&dn=test download this";
        assert_eq!(extract_clean_magnet(m3), Some("magnet:?xt=urn:btih:feb1f53aa5b3a1660d853c158710175f90d20074&dn=test%20download%20this".to_string()));

        let m4 = "magnet:?dn=test";
        assert_eq!(extract_clean_magnet(m4), None);

        // Test with raw spaces in display name
        let m5_input = "magnet:?xt=urn:btih:feb1f53aa5b3a1660d853c158710175f90d20074&dn=BanG Dream! &tr=http://nyaa.tracker.wf:7777/announce";
        let m5_expected = "magnet:?xt=urn:btih:feb1f53aa5b3a1660d853c158710175f90d20074&dn=BanG%20Dream!&tr=http://nyaa.tracker.wf:7777/announce";
        assert_eq!(
            extract_clean_magnet(m5_input),
            Some(m5_expected.to_string())
        );

        // Test with multiline input similar to the user's screenshot
        let m6_input = "magnet:?\nxt=urn:btih:feb1f53aa5b3a1660d853c158710175f90d20074&dn=%5BJMAX%5D%20%5B2026.03.21%5D%20BanG%20Dream%21%20%E3%83%8F%E3%83%BC%E3%83%9F%E3%83%83%E3%83%88%E3%83%BB%E3%83%8D%E3%83%BC%E3%83%A0%E3%83%BB%E3%83%96%E3%83%AB%E3%83%BC%20-%20\n%20%E8%A8%B1%E5%A9%9A%E3%83%BB%E3%83%9D%E3%83%BC%E3%83%88%E3%83%AC%E3%83%BC%E3%83%88%20%28Cover%29%20%5BFLAC%2096kHz%2F24bit%5D&tr=http%3A%2F%2Fnyaa.tracker.wf%3A7777%2Fannounce&tr=udp%3A%2F%2Fopen.stealth.si%3A80%2Fannounce";
        let m6_expected = "magnet:?xt=urn:btih:feb1f53aa5b3a1660d853c158710175f90d20074&dn=%5BJMAX%5D%20%5B2026.03.21%5D%20BanG%20Dream%21%20%E3%83%8F%E3%83%BC%E3%83%9F%E3%83%83%E3%83%88%E3%83%BB%E3%83%8D%E3%83%BC%E3%83%A0%E3%83%BB%E3%83%96%E3%83%AB%E3%83%BC%20-%20%20%E8%A8%B1%E5%A9%9A%E3%83%BB%E3%83%9D%E3%83%BC%E3%83%88%E3%83%AC%E3%83%BC%E3%83%88%20%28Cover%29%20%5BFLAC%2096kHz%2F24bit%5D&tr=http%3A%2F%2Fnyaa.tracker.wf%3A7777%2Fannounce&tr=udp%3A%2F%2Fopen.stealth.si%3A80%2Fannounce";
        assert_eq!(
            extract_clean_magnet(m6_input),
            Some(m6_expected.to_string())
        );
    }
}
