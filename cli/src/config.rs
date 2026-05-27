use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ratatui::style::Color;

// ── Theme ─────────────────────────────────────────────────────────────────────

#[derive(Clone, serde::Deserialize)]
pub(crate) struct ThemeConfig {
    #[serde(default = "default_accent")]
    pub(crate) accent: String,
    #[serde(default = "default_sel_fg")]
    pub(crate) sel_fg: String,
    #[serde(default = "default_fg_dim")]
    pub(crate) fg_dim: String,
    #[serde(default = "default_fg_xdim")]
    pub(crate) fg_xdim: String,
    #[serde(default = "default_green")]
    pub(crate) green: String,
    #[serde(default = "default_red")]
    pub(crate) red: String,
    #[serde(default = "default_yellow")]
    pub(crate) yellow: String,
    #[serde(default = "default_purple")]
    pub(crate) purple: String,
}

fn default_accent() -> String {
    "#e8b887".into()
}
fn default_sel_fg() -> String {
    "#101010".into()
}
fn default_fg_dim() -> String {
    "#A0A0A0".into()
}
fn default_fg_xdim() -> String {
    "#7E7E7E".into()
}
fn default_green() -> String {
    "#90b99f".into()
}
fn default_red() -> String {
    "#f5a191".into()
}
fn default_yellow() -> String {
    "#e6b99d".into()
}
fn default_purple() -> String {
    "#aca1cf".into()
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            accent: default_accent(),
            sel_fg: default_sel_fg(),
            fg_dim: default_fg_dim(),
            fg_xdim: default_fg_xdim(),
            green: default_green(),
            red: default_red(),
            yellow: default_yellow(),
            purple: default_purple(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) accent: Color,
    pub(crate) sel_fg: Color,
    pub(crate) fg_dim: Color,
    pub(crate) fg_xdim: Color,
    pub(crate) green: Color,
    pub(crate) red: Color,
    pub(crate) yellow: Color,
    pub(crate) purple: Color,
}

pub(crate) fn hex_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(255);
    Color::Rgb(r, g, b)
}

thread_local! {
    static THEME: std::cell::Cell<Option<Theme>> = std::cell::Cell::new(None);
}

pub(crate) fn theme() -> Theme {
    THEME.with(|c| c.get().expect("theme not initialized"))
}

pub(crate) fn set_theme(cfg: &ThemeConfig) {
    let t = Theme {
        accent: hex_color(&cfg.accent),
        sel_fg: hex_color(&cfg.sel_fg),
        fg_dim: hex_color(&cfg.fg_dim),
        fg_xdim: hex_color(&cfg.fg_xdim),
        green: hex_color(&cfg.green),
        red: hex_color(&cfg.red),
        yellow: hex_color(&cfg.yellow),
        purple: hex_color(&cfg.purple),
    };
    THEME.with(|c| c.set(Some(t)));
}

pub(crate) fn reload_theme_if_changed(mtime: &mut Option<SystemTime>) {
    let Some(path) = dirs_home().map(|h| h.join(".project-index.toml")) else {
        return;
    };
    let Ok(meta) = fs::metadata(&path) else {
        return;
    };
    let Ok(modified) = meta.modified() else {
        return;
    };
    if mtime.map_or(true, |last| modified > last) {
        *mtime = Some(modified);
        if let Ok(s) = fs::read_to_string(&path) {
            if let Ok(cfg) = toml::from_str::<Config>(&s) {
                set_theme(&cfg.theme);
            }
        }
    }
}

// ── Icons (Nerd Fonts) ────────────────────────────────────────────────────────

pub(crate) const I_BRANCH: &str = "\u{e0a0}";
pub(crate) const I_CHECK: &str = "\u{f00c}";
pub(crate) const I_CROSS: &str = "\u{f12a}";
pub(crate) const I_WARN: &str = "\u{f071}";
pub(crate) const I_BULLET: &str = "\u{f111}";
pub(crate) const I_COMMIT: &str = "\u{f1d3}";
pub(crate) const I_ISSUES: &str = "\u{f41b}";
pub(crate) const I_SETUP: &str = "\u{f013}";
pub(crate) const I_PROJECTS: &str = "\u{f07b}";
pub(crate) const I_MEMORY: &str = "\u{f0eb}";
pub(crate) const I_MCP: &str = "\u{f0c1}";
pub(crate) const I_PANE: &str = "\u{f120}";

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Clone, serde::Deserialize, Default)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) projects: ProjectsConfig,
    #[serde(default)]
    pub(crate) theme: ThemeConfig,
}

#[derive(Clone, serde::Deserialize, Default)]
pub(crate) struct ProjectsConfig {
    #[serde(default)]
    pub(crate) roots: Vec<String>,
    pub(crate) root: Option<String>, // legacy single-root compat
}

impl ProjectsConfig {
    pub(crate) fn effective_roots(&self) -> Vec<String> {
        if !self.roots.is_empty() {
            return self.roots.clone();
        }
        if let Some(r) = &self.root {
            return vec![r.clone()];
        }
        vec![]
    }
}

pub(crate) fn load_config() -> Config {
    let home = dirs_home().unwrap_or_default();
    let path = home.join(".project-index.toml");
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn sanitize_project_component(project_path: &Path) -> String {
    project_path
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

pub(crate) fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
