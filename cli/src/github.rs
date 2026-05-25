use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::config::dirs_home;
use crate::project::RepoMeta;

// ── GitHub metadata cache ─────────────────────────────────────────────────────

pub(crate) fn meta_cache_path() -> PathBuf {
    dirs_home()
        .unwrap_or_default()
        .join(".project-index")
        .join("cache.json")
}

pub(crate) fn load_meta_cache() -> HashMap<String, RepoMeta> {
    let path = meta_cache_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_meta_cache(cache: &HashMap<String, RepoMeta>) {
    let path = meta_cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(cache) {
        let _ = fs::write(&path, json);
    }
}

pub(crate) fn refresh_project_meta(repo: &str) -> Option<RepoMeta> {
    if !repo.contains('/') {
        return None;
    }
    let out = Command::new("gh")
        .args([
            "repo",
            "view",
            repo,
            "--json",
            "primaryLanguage,repositoryTopics,pushedAt,issues",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let language = v["primaryLanguage"]["name"].as_str().map(|s| s.to_string());
    let topics = v["repositoryTopics"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|t| t["name"].as_str().map(|s| s.to_string()))
        .collect();
    let pushed_at = v["pushedAt"].as_str().map(|s| s.to_string());
    let open_issues = v["issues"]["totalCount"].as_u64().map(|n| n as u32);
    Some(RepoMeta {
        language,
        topics,
        pushed_at,
        open_issues,
    })
}

pub(crate) fn lang_short(lang: &str) -> &str {
    match lang {
        "TypeScript" => "TS",
        "JavaScript" => "JS",
        "Rust" => "RS",
        "Go" => "Go",
        "Python" => "Py",
        "Ruby" => "Rb",
        "CSS" => "CS",
        "HTML" => "HT",
        "Shell" => "SH",
        "Svelte" => "SV",
        "Solidity" => "So",
        "Nix" => "Nx",
        other => {
            if other.len() >= 2 {
                &other[..2]
            } else {
                other
            }
        }
    }
}

pub(crate) fn relative_date(iso: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let parts: Vec<u64> = iso
        .splitn(2, 'T')
        .next()
        .unwrap_or("")
        .split('-')
        .filter_map(|s| s.parse().ok())
        .collect();
    if parts.len() < 3 {
        return String::new();
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);
    let month_days = [0u64, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let month_sum: u64 = (1..m as usize).map(|i| month_days[i]).sum();
    let days_epoch = (y - 1970) * 365 + (y - 1970) / 4 + month_sum + d - 1;
    let diff = now.saturating_sub(days_epoch * 86400) / 86400;
    match diff {
        0 => "today".to_string(),
        1..=6 => format!("{}d", diff),
        7..=29 => format!("{}w", diff / 7),
        30..=364 => format!("{}mo", diff / 30),
        _ => format!("{}y", diff / 365),
    }
}

// ── Avatar (chafa) ────────────────────────────────────────────────────────────

pub(crate) fn avatar_dir() -> PathBuf {
    dirs_home()
        .unwrap_or_default()
        .join(".project-index")
        .join("avatars")
}

pub(crate) fn fetch_avatar(owner: &str) -> Option<String> {
    let dir = avatar_dir();
    let _ = fs::create_dir_all(&dir);
    let png = dir.join(format!("{owner}.png"));

    if !png.exists() {
        let url = format!("https://github.com/{owner}.png?size=128");
        let ok = Command::new("curl")
            .args(["-s", "-L", "-o", png.to_str().unwrap_or(""), &url])
            .status()
            .ok()?
            .success();
        if !ok {
            return None;
        }
    }

    let out = Command::new("chafa")
        .args([
            "--size",
            "20x10",
            "--format",
            "symbols",
            png.to_str().unwrap_or(""),
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub(crate) fn ansi_to_lines(s: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut style = Style::default();
    let mut text = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            let mut seq = String::new();
            for nc in chars.by_ref() {
                if nc == 'm' {
                    break;
                }
                seq.push(nc);
            }
            if !text.is_empty() {
                spans.push(Span::styled(text.clone(), style));
                text.clear();
            }
            style = apply_sgr(style, &seq);
        } else if c == '\n' {
            if !text.is_empty() {
                spans.push(Span::styled(text.clone(), style));
                text.clear();
            }
            lines.push(Line::from(spans.clone()));
            spans.clear();
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        spans.push(Span::styled(text, style));
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

fn apply_sgr(mut style: Style, seq: &str) -> Style {
    let codes: Vec<u32> = seq.split(';').filter_map(|s| s.parse().ok()).collect();
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            39 => style = style.fg(Color::Reset),
            49 => style = style.bg(Color::Reset),
            38 if codes.get(i + 1) == Some(&2) && i + 4 < codes.len() => {
                style = style.fg(Color::Rgb(
                    codes[i + 2] as u8,
                    codes[i + 3] as u8,
                    codes[i + 4] as u8,
                ));
                i += 4;
            }
            48 if codes.get(i + 1) == Some(&2) && i + 4 < codes.len() => {
                style = style.bg(Color::Rgb(
                    codes[i + 2] as u8,
                    codes[i + 3] as u8,
                    codes[i + 4] as u8,
                ));
                i += 4;
            }
            38 if codes.get(i + 1) == Some(&5) && i + 2 < codes.len() => {
                style = style.fg(ansi256(codes[i + 2] as u8));
                i += 2;
            }
            48 if codes.get(i + 1) == Some(&5) && i + 2 < codes.len() => {
                style = style.bg(ansi256(codes[i + 2] as u8));
                i += 2;
            }
            _ => {}
        }
        i += 1;
    }
    style
}

fn ansi256(n: u8) -> Color {
    match n {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::White,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        16..=231 => {
            let v = n - 16;
            let b = (v % 6) * 51;
            let g = ((v / 6) % 6) * 51;
            let r = (v / 36) * 51;
            Color::Rgb(r, g, b)
        }
        232..=255 => {
            let v = (n - 232) * 10 + 8;
            Color::Rgb(v, v, v)
        }
    }
}
