//! `nekotrail theme`: set the app's colour theme from the terminal, so any agent can make you
//! one. The app does the real work (src/themes.ts works out every colour and keeps text
//! readable); this side checks the shape, so a bad theme gets a clear error in the terminal
//! instead of silently doing nothing.

use serde_json::Value;

/// The presets, id and name, in the same order as src/themes.ts (tests/themes.test.ts checks).
pub const PRESETS: &[(&str, &str)] = &[
    ("dracula", "Dracula"),
    ("tokyo-night", "Tokyo Night"),
    ("catppuccin-mocha", "Catppuccin Mocha"),
    ("catppuccin-latte", "Catppuccin Latte"),
    ("nord", "Nord"),
    ("gruvbox-dark", "Gruvbox Dark"),
    ("gruvbox-light", "Gruvbox Light"),
    ("one-dark", "One Dark"),
    ("rose-pine", "Rosé Pine"),
    ("solarized-dark", "Solarized Dark"),
    ("solarized-light", "Solarized Light"),
    ("github-dark", "GitHub Dark"),
    ("github-light", "GitHub Light"),
];

/// A theme file is small. Anything bigger is not a theme.
pub const MAX_BYTES: usize = 16 * 1024;

pub const HELP: &str = r##"nekotrail theme: change NekoTrail's colours.

Usage:
  nekotrail theme <name>         Use a preset (list below), or moonlight for the default
  nekotrail theme <file.json>    Use your own theme from a file
  nekotrail theme -              The same, read from standard input

A theme is JSON. Only background, text and accent are needed; NekoTrail works out the rest
and keeps all text readable. Colours are hex.

  {
    "name": "Midnight Mint",
    "mode": "dark",
    "colors": {
      "background": "#0f1419",
      "text": "#e6e1cf",
      "accent": "#5ccfe6",
      "sidebar": "#0b0f13",
      "surface": "#141a21",
      "muted": "#a0a8b0",
      "border": "#1f2730",
      "green": "#91d076",
      "red": "#f28779",
      "yellow": "#ffd173",
      "orange": "#ffad66",
      "purple": "#d4bfff"
    },
    "terminal": {
      "background": "#0f1419",
      "foreground": "#e6e1cf",
      "cursor": "#5ccfe6",
      "ansi": ["#1f2730", "#f28779", "#91d076", "#ffd173", "#5ccfe6", "#d4bfff", "#95e6cb", "#c7c7c7",
               "#4f5a66", "#ff9e94", "#a6e08a", "#ffe08f", "#7fdcf0", "#e0d0ff", "#aaf0d8", "#ffffff"]
    }
  }

mode is dark or light (guessed from the background if left out). terminal is optional; ansi is
the 16 terminal colours in order, black red green yellow blue magenta cyan white, then bright.
You can also copy a theme in Settings, Appearance and paste it back there.

Presets: "##;

/// The help text with the preset list filled in.
pub fn help() -> String {
    let names: Vec<&str> = PRESETS.iter().map(|(id, _)| *id).collect();
    format!("{HELP}moonlight, {}.\n", names.join(", "))
}

fn is_hex(v: &Value) -> bool {
    let Some(s) = v.as_str() else { return false };
    let s = s.trim();
    let h = s.strip_prefix('#').unwrap_or("");
    s.starts_with('#') && (h.len() == 3 || h.len() == 6) && h.chars().all(|c| c.is_ascii_hexdigit())
}

/// The name of a preset id, or `moonlight`.
pub fn preset_name(id: &str) -> Option<&'static str> {
    if id == "moonlight" {
        return Some("Moonlight");
    }
    PRESETS.iter().find(|(p, _)| *p == id).map(|(_, n)| *n)
}

/// Checks a theme the way the app reads it: a preset id, or an object whose colours (in `colors`,
/// or at the top level) have a hex background, text and accent. Returns the name to show.
pub fn validate(theme: &Value) -> Result<String, String> {
    if let Some(id) = theme.as_str() {
        let id = id.trim().to_ascii_lowercase();
        return preset_name(&id)
            .map(str::to_string)
            .ok_or_else(|| format!("There's no theme called \"{id}\". Run nekotrail theme to see the presets."));
    }
    let Some(obj) = theme.as_object() else { return Err("A theme is a JSON object with colors. Run nekotrail theme for the format.".into()) };
    if serde_json::to_string(theme).map(|s| s.len()).unwrap_or(usize::MAX) > MAX_BYTES {
        return Err("That theme is too big. A theme is a few colours.".into());
    }
    let colors = obj.get("colors").and_then(Value::as_object).unwrap_or(obj);
    let pick = |keys: &[&str]| keys.iter().find_map(|k| colors.get(*k));
    for (need, keys) in [("background", &["background", "bg"][..]), ("text", &["text", "foreground", "fg"]), ("accent", &["accent", "primary"])] {
        match pick(keys) {
            None => return Err(format!("The theme needs a {need} colour.")),
            Some(v) if !is_hex(v) => return Err(format!("{need} must be a hex colour like #1e1e2e.")),
            _ => {}
        }
    }
    let name = obj.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Custom");
    Ok(name.chars().take(40).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn presets_and_moonlight_by_name() {
        assert_eq!(validate(&json!("dracula")), Ok("Dracula".into()));
        assert_eq!(validate(&json!("  Nord ")), Ok("Nord".into()));
        assert_eq!(validate(&json!("moonlight")), Ok("Moonlight".into()));
        assert!(validate(&json!("vaporwave")).unwrap_err().contains("no theme called \"vaporwave\""));
    }

    #[test]
    fn own_theme_needs_three_hex_colours() {
        let ok = json!({"name": "Mint", "colors": {"background": "#0f1419", "text": "#fff", "accent": "#5ccfe6"}});
        assert_eq!(validate(&ok), Ok("Mint".into()));
        let flat = json!({"bg": "#000000", "fg": "#ffffff", "primary": "#ff8800"});
        assert_eq!(validate(&flat), Ok("Custom".into()));
        assert_eq!(validate(&json!({"colors": {"background": "#000", "text": "#fff"}})), Err("The theme needs a accent colour.".into()));
        assert_eq!(validate(&json!({"colors": {"background": "black", "text": "#fff", "accent": "#fff"}})), Err("background must be a hex colour like #1e1e2e.".into()));
        assert!(validate(&json!([1, 2])).is_err());
        assert!(validate(&json!({"colors": {"background": "#000", "text": "#fff", "accent": "#fff;}"}})).is_err());
    }

    #[test]
    fn huge_input_is_refused() {
        let big = json!({"colors": {"background": "#000", "text": "#fff", "accent": "#fff"}, "pad": "x".repeat(MAX_BYTES)});
        assert!(validate(&big).unwrap_err().contains("too big"));
    }

    #[test]
    fn the_example_in_the_help_is_a_valid_theme() {
        let start = HELP.find('{').unwrap();
        let end = HELP.rfind('}').unwrap();
        let example: Value = serde_json::from_str(&HELP[start..=end]).expect("help example parses");
        assert_eq!(validate(&example), Ok("Midnight Mint".into()));
        assert_eq!(example["terminal"]["ansi"].as_array().unwrap().len(), 16);
    }

    #[test]
    fn help_lists_every_preset() {
        let h = help();
        for (id, _) in PRESETS {
            assert!(h.contains(id), "{id} missing from help");
        }
    }
}
