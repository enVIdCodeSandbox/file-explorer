//! The color a window is painted with before its page has loaded, so a window never flashes white (or the wrong
//! color) while it starts. It follows the person's theme; "system" follows the system's light or dark setting.

// Only desktop windows are painted ahead of their page.
#![cfg_attr(not(desktop), allow(dead_code))]

use tauri::window::Color;

/// Background of each built-in theme (the first swatch of `src/util/themes.ts`; a test keeps them equal).
const BACKGROUNDS: &[(&str, u32)] = &[
    ("dark", 0x1c1c1c), ("light", 0xeef1f5), ("midnight", 0x0b1220), ("graphite", 0x121212), ("nord", 0x2e3440),
    ("dracula", 0x21222c), ("solarized-dark", 0x002b36), ("forest", 0x10190f), ("solarized-light", 0xeee8d5),
    ("sepia", 0xe9dfcc), ("rose", 0xf6e8ee), ("win11-dark", 0x202020), ("win11-light", 0xf3f3f3), ("win10-dark", 0x191919),
    ("win10-light", 0xf0f0f0), ("win10-slate", 0x141a21), ("mac-dark", 0x1e1e1e),
    ("mac-light", 0xececec), ("adwaita-dark", 0x1e1e1e), ("adwaita-light", 0xfafafb), ("material-dark", 0x131314),
    ("material-light", 0xffffff), ("oneui-dark", 0x000000), ("oneui-light", 0xf2f2f2), ("ios-dark", 0x000000),
    ("ios-light", 0xf2f2f7), ("contrast-dark", 0x000000), ("contrast-light", 0xffffff),
];

/// The built-in theme a saved setting stands for.
pub fn resolve(saved: &str, system_dark: bool) -> &str {
    if saved == "system" {
        return if system_dark { "dark" } else { "light" };
    }
    if BACKGROUNDS.iter().any(|(id, _)| *id == saved) {
        saved
    } else {
        "dark"
    }
}

pub fn background(saved: &str, system_dark: bool) -> Color {
    let id = resolve(saved, system_dark);
    let rgb = BACKGROUNDS.iter().find(|(i, _)| *i == id).map_or(0x1c1c1c, |(_, c)| *c);
    Color((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255)
}

/// The system's light/dark setting as the window sees it (dark when it cannot tell).
pub fn system_dark(window: &tauri::WebviewWindow) -> bool {
    !matches!(window.theme(), Ok(tauri::Theme::Light))
}

/// The color for a window about to open: the saved theme, or the system's when set to follow it.
pub fn for_new_window(app: &tauri::AppHandle) -> Color {
    use tauri::Manager;
    let saved = app.try_state::<crate::state::Shared>().map(|s| s.settings.read().appearance.theme.clone()).unwrap_or_default();
    let dark = app.get_webview_window("main").map_or(true, |w| system_dark(&w));
    background(&saved, dark)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_saved_theme_or_the_system() {
        assert_eq!(resolve("system", true), "dark");
        assert_eq!(resolve("system", false), "light");
        assert_eq!(resolve("nord", false), "nord");
        assert_eq!(resolve("something-removed", false), "dark");
        let Color(r, g, b, a) = background("light", true);
        assert_eq!((r, g, b, a), (0xee, 0xf1, 0xf5, 255));
    }

    #[test]
    fn matches_the_themes_the_page_paints() {
        // Every theme in themes.ts has the same first swatch here, so the window and the page start the same color.
        let ts = include_str!("../../src/util/themes.ts");
        let mut seen = 0;
        for line in ts.lines().filter(|l| l.contains("swatch: [\"")) {
            let id = line.split("id: \"").nth(1).and_then(|r| r.split('"').next()).unwrap();
            let bg = line.split("swatch: [\"").nth(1).and_then(|r| r.split('"').next()).unwrap();
            let want = u32::from_str_radix(bg.trim_start_matches('#'), 16).unwrap();
            let have = BACKGROUNDS.iter().find(|(i, _)| *i == id).unwrap_or_else(|| panic!("theme {id} is missing")).1;
            assert_eq!(have, want, "background of {id}");
            seen += 1;
        }
        assert_eq!(seen, BACKGROUNDS.len());
    }
}
