//! snapcode-lsp: language server offering a "Snap selection" code action that
//! renders the selected code to a PNG, plus a `render` CLI subcommand.

mod clipboard;
mod server;
mod settings;
mod slice;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use settings::Settings;

#[derive(Parser)]
#[command(
    name = "snapcode-lsp",
    version,
    about = "Beautiful code screenshots for Zed"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the language server over stdio (default).
    Lsp,
    /// Render code to a PNG from the command line.
    Render(RenderArgs),
    /// List available themes.
    Themes,
}

#[derive(clap::Args)]
struct RenderArgs {
    /// Source file (used for syntax detection and title; read when --text is absent).
    #[arg(long)]
    file: Option<PathBuf>,
    /// Code to render (e.g. "$ZED_SELECTED_TEXT"). Falls back to the file content.
    #[arg(long)]
    text: Option<String>,
    /// Language id / name / extension (overrides detection from --file).
    #[arg(long)]
    lang: Option<String>,
    /// Output PNG path. Defaults to `<output_dir>/<file>-<timestamp>.png`.
    #[arg(long, short)]
    out: Option<PathBuf>,
    /// Line number of the first line; enables line numbers.
    #[arg(long)]
    start_line: Option<usize>,
    /// Settings as JSON, same shape as Zed's `initialization_options`.
    #[arg(long)]
    config: Option<String>,
    /// Theme name (overrides --config).
    #[arg(long)]
    theme: Option<String>,
    /// Copy the image to the clipboard.
    #[arg(long)]
    copy: bool,
}

fn main() -> Result<()> {
    match Cli::parse().command.unwrap_or(Cmd::Lsp) {
        Cmd::Lsp => server::run(),
        Cmd::Themes => {
            for t in snap_core::available_themes() {
                println!("{t}");
            }
            Ok(())
        }
        Cmd::Render(args) => render_cli(args),
    }
}

fn render_cli(args: RenderArgs) -> Result<()> {
    let mut settings = match &args.config {
        Some(json) => serde_json::from_str::<Settings>(json).context("invalid --config JSON")?,
        None => Settings::default(),
    };
    if let Some(theme) = args.theme {
        settings.render.theme = theme;
    }
    if let Some(n) = args.start_line {
        settings.render.line_numbers = true;
        settings.render.start_line = n;
    }
    let code = match (&args.text, &args.file) {
        (Some(t), _) if !t.is_empty() => t.clone(),
        (_, Some(f)) => {
            std::fs::read_to_string(f).with_context(|| format!("reading {}", f.display()))?
        }
        _ => anyhow::bail!("pass --text or --file"),
    };
    let file_name = args
        .file
        .as_ref()
        .and_then(|f| f.file_name())
        .map(|f| f.to_string_lossy().into_owned());
    let snap = server::snap(
        &code,
        args.lang.as_deref(),
        file_name.as_deref(),
        &settings,
        args.out,
        args.copy,
        None,
    )?;
    println!("{}", snap.message);
    Ok(())
}
