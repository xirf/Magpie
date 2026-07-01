use serde_json::Value;
use std::collections::BTreeMap as HashMap;
use std::sync::OnceLock;

static TRANSLATIONS: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();

fn init_translations() -> HashMap<String, HashMap<String, String>> {
    let mut map = HashMap::new();

    let locales = [
        ("en", include_str!("locales/en.json")),
        ("es", include_str!("locales/es.json")),
        ("it", include_str!("locales/it.json")),
        ("pt", include_str!("locales/pt.json")),
        ("ru", include_str!("locales/ru.json")),
        ("uk", include_str!("locales/uk.json")),
    ];

    for (lang, content) in locales {
        if let Ok(Value::Object(obj)) = serde_json::from_str(content) {
            let mut lang_map = HashMap::new();
            for (k, v) in obj {
                if let Value::String(s) = v {
                    lang_map.insert(k, s);
                }
            }
            map.insert(lang.to_string(), lang_map);
        }
    }

    map
}

pub fn t(key: &str, locale: &str, vars: Option<&HashMap<String, String>>) -> String {
    let translations = TRANSLATIONS.get_or_init(init_translations);

    // Get the translation for the locale, fallback to 'en', then fallback to key
    let lang_map = translations.get(locale).or_else(|| translations.get("en"));

    let mut template = match lang_map {
        Some(m) => m.get(key).cloned().unwrap_or_else(|| key.to_string()),
        None => key.to_string(),
    };

    if let Some(vars_map) = vars {
        for (k, v) in vars_map {
            let placeholder = format!("{{{}}}", k);
            template = template.replace(&placeholder, v);
        }
    }

    template
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_t_simple() {
        // English
        assert_eq!(
            t("You are not authorized to use this bot", "en", None),
            "You are not authorized to use this bot"
        );
        // Italian
        assert_eq!(
            t("You are not authorized to use this bot", "it", None),
            "Non sei autorizzato ad usare questo bot"
        );
        // Spanish
        assert_eq!(
            t("You are not authorized to use this bot", "es", None),
            "No estás autorizado para usar este bot"
        );
    }

    #[test]
    fn test_t_fallback() {
        assert_eq!(
            t("This key does not exist anywhere", "en", None),
            "This key does not exist anywhere"
        );
        assert_eq!(
            t("This key does not exist anywhere", "it", None),
            "This key does not exist anywhere"
        );
    }

    #[test]
    fn test_t_substitutions() {
        let mut vars = HashMap::new();
        vars.insert(
            "name".to_string(),
            "ubuntu-24.04-desktop-amd64.iso".to_string(),
        );

        // English
        assert_eq!(
            t(
                "Torrent {name} has finished downloading!",
                "en",
                Some(&vars)
            ),
            "Torrent ubuntu-24.04-desktop-amd64.iso has finished downloading!"
        );

        // Italian
        assert_eq!(
            t(
                "Torrent {name} has finished downloading!",
                "it",
                Some(&vars)
            ),
            "Il torrent ubuntu-24.04-desktop-amd64.iso è stato scaricato!"
        );

        // Multiple variables
        let mut stats_vars = HashMap::new();
        stats_vars.insert("cpu_usage".to_string(), "12".to_string());
        stats_vars.insert("cpu_temp".to_string(), "45".to_string());
        stats_vars.insert("free_memory".to_string(), "8.5 GB".to_string());
        stats_vars.insert("total_memory".to_string(), "16 GB".to_string());
        stats_vars.insert("memory_percent".to_string(), "46".to_string());
        stats_vars.insert("disk_used".to_string(), "450 GB".to_string());
        stats_vars.insert("disk_total".to_string(), "1 TB".to_string());
        stats_vars.insert("disk_percent".to_string(), "45".to_string());

        let stats_text = t(
            "**============SYSTEM============**\n**CPU Usage:** {cpu_usage}%\n**CPU Temp:** {cpu_temp}°C\n**Free Memory:** {free_memory} of {total_memory} ({memory_percent}%)\n**Disks usage:** {disk_used} of {disk_total} ({disk_percent}%)",
            "it",
            Some(&stats_vars)
        );

        assert!(stats_text.contains("SISTEMA"));
        assert!(stats_text.contains("**Utilizzo CPU:** 12%"));
        assert!(stats_text.contains("**Temperatura CPU:** 45°C"));
        assert!(stats_text.contains("**Memoria libera:** 8.5 GB"));
        assert!(stats_text.contains("**Utilizzo disco:** 450 GB"));
    }
}
