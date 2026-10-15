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

/// The bar's config file, `~/.config/patina-bar/config.toml`.
pub fn config_path() -> Option<std::path::PathBuf> {
    Some(dirs_config()?.join("patina-bar").join("config.toml"))
}

fn config_value(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(config_path()?).ok()?;
    top_level(&text, key)
}

/// The desktop's wallpaper, an image file; `None` for the skin's colour.
pub fn wallpaper() -> Option<std::path::PathBuf> {
    config_value("wallpaper")
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
}

/// Sets the top-level `key` (`skin`, `wallpaper`) in the bar's config file,
/// keeping the rest of the file, and making the file if there's none. The
/// bar and the wallpaper notice; pieces pick it up when they next open.
pub fn set(key: &str, value: &str) -> Result<(), String> {
    let path = config_path().ok_or("no home folder")?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, with_top_level(&text, key, value))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// `text` with the top-level `key` set to the string `value`: the line
/// that sets it replaced, or a new one before the first [section].
pub fn with_top_level(text: &str, key: &str, value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    let line = format!("{key} = \"{escaped}\"");
    let mut out = Vec::new();
    let mut done = false;
    for l in text.lines() {
        let trimmed = l.trim();
        if !done && trimmed.starts_with('[') {
            out.push(line.clone());
            done = true;
        }
        let sets_key = trimmed
            .split_once('=')
            .is_some_and(|(k, _)| k.trim() == key);
        if !done && sets_key {
            out.push(line.clone());
            done = true;
        } else {
            out.push(l.to_string());
        }
    }
    if !done {
        out.push(line);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
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

    #[test]
    fn sets_a_top_level_key_keeping_the_rest() {
        let text = "# bar\nskin = \"Default\"  # old\nheight = 30\n[other]\nskin = \"x\"\n";
        assert_eq!(
            with_top_level(text, "skin", "Steam Classic"),
            "# bar\nskin = \"Steam Classic\"\nheight = 30\n[other]\nskin = \"x\"\n"
        );
        assert_eq!(
            with_top_level("height = 30\n[a]\nb = 1\n", "skin", "Aqua"),
            "height = 30\nskin = \"Aqua\"\n[a]\nb = 1\n"
        );
        assert_eq!(with_top_level("", "skin", "A \"q\""), "skin = \"A \\\"q\\\"\"\n");
        let set = with_top_level("# none yet\n", "skin", "Aqua");
        assert_eq!(top_level(&set, "skin").as_deref(), Some("Aqua"));
    }
}
