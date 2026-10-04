//! Syntax highlighting via syntect + two-face (bat's syntax/theme collection).

use std::sync::OnceLock;

use anyhow::Result;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color, FontStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName};

use crate::Rgba;

pub struct Span {
    pub text: String,
    pub color: Rgba,
    pub bold: bool,
}

pub struct Highlighted {
    pub lines: Vec<Vec<Span>>,
    pub background: Rgba,
    pub foreground: Rgba,
}

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

fn themes() -> &'static EmbeddedLazyThemeSet {
    static SET: OnceLock<EmbeddedLazyThemeSet> = OnceLock::new();
    SET.get_or_init(two_face::theme::extra)
}

/// Theme names accepted by [`resolve_theme_name`] (canonical spelling).
pub fn available_themes() -> Vec<&'static str> {
    EmbeddedLazyThemeSet::theme_names()
        .iter()
        .filter(|t| {
            !matches!(
                t,
                EmbeddedThemeName::Ansi | EmbeddedThemeName::Base16 | EmbeddedThemeName::Base16_256
            )
        })
        .map(|t| t.as_name())
        .collect()
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Maps a user-supplied theme name (case/punctuation-insensitive, with a few
/// common aliases) to a canonical theme name.
pub fn resolve_theme_name(name: &str) -> Option<&'static str> {
    let n = normalize(name);
    let alias = match n.as_str() {
        "onedark" | "atomonedark" => Some(EmbeddedThemeName::TwoDark),
        "onelight" | "atomonelight" => Some(EmbeddedThemeName::OneHalfLight),
        "monokai" => Some(EmbeddedThemeName::MonokaiExtended),
        "github" | "githublight" => Some(EmbeddedThemeName::Github),
        "gruvbox" => Some(EmbeddedThemeName::GruvboxDark),
        "catppuccin" => Some(EmbeddedThemeName::CatppuccinMocha),
        "solarized" => Some(EmbeddedThemeName::SolarizedDark),
        _ => None,
    };
    if let Some(a) = alias {
        return Some(a.as_name());
    }
    available_themes().into_iter().find(|t| normalize(t) == n)
}

fn theme_by_name(name: &str) -> &'static Theme {
    let id = EmbeddedLazyThemeSet::theme_names()
        .iter()
        .find(|t| t.as_name() == name)
        .copied()
        .unwrap_or(EmbeddedThemeName::Dracula);
    themes().get(id)
}

/// Maps LSP language ids / Zed language names to a file extension syntect knows.
fn lang_to_token(lang: &str) -> &str {
    match lang.to_ascii_lowercase().as_str() {
        "typescript" => "ts",
        "typescriptreact" | "tsx" => "tsx",
        "javascript" => "js",
        "javascriptreact" | "jsx" => "jsx",
        "python" => "py",
        "rust" => "rs",
        "shellscript" | "shell script" | "shell" | "bash" | "zsh" | "fish" => "sh",
        "csharp" | "c#" => "cs",
        "cpp" | "c++" => "cpp",
        "objective-c" | "objc" => "m",
        "ruby" => "rb",
        "kotlin" => "kt",
        "markdown" => "md",
        "elixir" => "ex",
        "erlang" => "erl",
        "haskell" => "hs",
        "ocaml" => "ml",
        "perl" => "pl",
        "powershell" => "ps1",
        "clojure" => "clj",
        "julia" => "jl",
        "terraform" | "hcl" => "tf",
        "jsonc" | "json5" => "json",
        "dockerfile" => "Dockerfile",
        "makefile" | "make" => "Makefile",
        "plaintext" | "plain text" | "text" => "txt",
        _ => lang,
    }
}

fn find_syntax<'a>(
    ss: &'a SyntaxSet,
    lang: Option<&str>,
    file_name: Option<&str>,
) -> &'a SyntaxReference {
    if let Some(file) = file_name {
        let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
        let ext = base.rsplit_once('.').map(|(_, e)| e).unwrap_or(base);
        if let Some(s) = ss
            .find_syntax_by_extension(ext)
            .or_else(|| ss.find_syntax_by_extension(base))
        {
            return s;
        }
    }
    if let Some(lang) = lang {
        let token = lang_to_token(lang);
        if let Some(s) = ss.find_syntax_by_token(token) {
            return s;
        }
        if let Some(s) = ss
            .syntaxes()
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(lang))
        {
            return s;
        }
    }
    ss.find_syntax_plain_text()
}

fn rgba(c: Color) -> Rgba {
    Rgba(c.r, c.g, c.b, c.a)
}

pub fn highlight(
    lines: &[String],
    lang: Option<&str>,
    file_name: Option<&str>,
    theme_name: &str,
) -> Result<Highlighted> {
    let ss = syntaxes();
    let theme = theme_by_name(theme_name);
    let syntax = find_syntax(ss, lang, file_name);
    let mut h = HighlightLines::new(syntax, theme);

    let background = theme
        .settings
        .background
        .map(rgba)
        .unwrap_or(Rgba(0x28, 0x2A, 0x36, 255));
    let foreground = theme
        .settings
        .foreground
        .map(rgba)
        .unwrap_or(Rgba(0xF8, 0xF8, 0xF2, 255));

    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let with_nl = format!("{line}\n");
        let ranges = h.highlight_line(&with_nl, ss)?;
        let spans = ranges
            .into_iter()
            .filter_map(|(style, text)| {
                let text = text.trim_end_matches(['\n', '\r']);
                (!text.is_empty()).then(|| Span {
                    text: text.to_string(),
                    color: Rgba(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                        255,
                    ),
                    bold: style.font_style.contains(FontStyle::BOLD),
                })
            })
            .collect();
        out.push(spans);
    }
    Ok(Highlighted {
        lines: out,
        background: Rgba(background.0, background.1, background.2, 255),
        foreground,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_aliases() {
        assert_eq!(resolve_theme_name("one dark"), Some("TwoDark"));
        assert_eq!(resolve_theme_name("dracula"), Some("Dracula"));
        assert_eq!(
            resolve_theme_name("Solarized Light"),
            Some("Solarized (light)")
        );
        assert_eq!(resolve_theme_name("nope"), None);
    }

    #[test]
    fn finds_syntaxes() {
        let ss = syntaxes();
        for (lang, file, want) in [
            (Some("typescript"), None, "TypeScript"),
            (Some("typescriptreact"), None, "TypeScriptReact"),
            (Some("rust"), None, "Rust"),
            (Some("Python"), None, "Python"),
            (Some("Shell Script"), None, "Bourne Again Shell (bash)"),
            (None, Some("/a/b/main.go"), "Go"),
            (Some("unknown"), Some("x.toml"), "TOML"),
            (Some("whatever"), None, "Plain Text"),
        ] {
            assert_eq!(find_syntax(ss, lang, file).name, want, "{lang:?} {file:?}");
        }
    }
}
