//! The bar's skin and edge, for shell pieces the bar opens (the launcher,
//! the volume menu) so they match it and open from it. The bar passes them
//! as PATINA_SKIN and PATINA_BAR_EDGE; when something else starts a piece
//! (the compositor, for the Super key or a volume key), they come from the
//! bar's config file instead.

/// The bar's skin name, if any is set.
pub fn skin() -> Option<String> {
    std::env::var("PATINA_SKIN")
        .ok()
        .or_else(|| config_value("skin"))
}

/// Whether the bar is at the top of the screen (the default is the bottom).
pub fn at_top() -> bool {
    std::env::var("PATINA_BAR_EDGE")
        .ok()
        .or_else(|| config_value("position"))
        .as_deref()
        == Some("top")
}

fn config_value(key: &str) -> Option<String> {
    let path = dirs_config()?.join("patina-bar").join("config.toml");
    let text = std::fs::read_to_string(path).ok()?;
    top_level(&text, key)
}

/// `key = "value"` at the top of a TOML file, before any [section].
pub fn top_level(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            return None;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() == key {
            let v = v.split('#').next()?.trim();
            return Some(v.trim_matches('"').to_string());
        }
    }
    None
}

fn dirs_config() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_top_level_keys_only() {
        let text = "# bar\nposition = \"top\"   # or bottom\nskin = \"Steam Classic\"\n[other]\nskin = \"x\"\n";
        assert_eq!(top_level(text, "position").as_deref(), Some("top"));
        assert_eq!(top_level(text, "skin").as_deref(), Some("Steam Classic"));
        assert_eq!(top_level("[a]\nskin = \"x\"", "skin"), None);
        assert_eq!(top_level("", "skin"), None);
    }
}
