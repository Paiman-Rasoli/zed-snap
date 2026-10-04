//! LSP server: tracks open documents, offers the snap code action, executes it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use tower_lsp::jsonrpc;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::clipboard;
use crate::settings::Settings;
use crate::slice;

pub const SNAP_COMMAND: &str = "snapcode.snap";

struct Doc {
    text: String,
    language_id: String,
}

struct Backend {
    client: Client,
    docs: RwLock<HashMap<Url, Doc>>,
    settings: RwLock<Arc<Settings>>,
}

#[derive(Deserialize)]
struct SnapArgs {
    uri: Url,
    range: Range,
    #[serde(default)]
    line_numbers: Option<bool>,
}

pub struct Snap {
    pub path: Option<PathBuf>,
    pub message: String,
}

/// Renders `code`, saves and/or copies it according to `settings`.
pub fn snap(
    code: &str,
    lang: Option<&str>,
    file_name: Option<&str>,
    settings: &Settings,
    out: Option<PathBuf>,
    copy: bool,
    start_line: Option<usize>,
) -> Result<Snap> {
    let mut opts = settings.render.clone();
    if settings.show_filename {
        opts.title = file_name.map(str::to_string);
    }
    if let Some(n) = start_line {
        opts.start_line = n;
    }
    let rendered = snap_core::render(code, lang, file_name, &opts)?;
    let (w, h) = rendered.image.dimensions();

    let mut notes = Vec::new();
    let mut path = None;
    if settings.save_file || out.is_some() {
        let target = match out {
            Some(p) => p,
            None => {
                let dir = settings.output_dir();
                let stem = file_name
                    .and_then(|f| f.rsplit(['/', '\\']).next())
                    .map(|f| f.replace('.', "_"))
                    .unwrap_or_else(|| "snap".into());
                dir.join(format!(
                    "{stem}-{}.png",
                    chrono::Local::now().format("%Y%m%d-%H%M%S")
                ))
            }
        };
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&target, snap_core::encode_png(&rendered.image)?)
            .with_context(|| format!("writing {}", target.display()))?;
        notes.push(format!("saved to {}", target.display()));
        path = Some(target);
    }
    let mut copied = false;
    if copy {
        match clipboard::copy_image(w, h, rendered.image.into_raw()) {
            Ok(()) => copied = true,
            Err(e) => notes.push(format!("clipboard failed: {e}")),
        }
    }
    if rendered.truncated {
        notes.push(format!(
            "selection capped to {} lines × {} columns",
            opts.max_lines, opts.max_columns
        ));
    }
    if rendered.unknown_theme {
        notes.push(format!(
            "unknown theme `{}`, used {}",
            opts.theme, rendered.theme
        ));
    }
    let head = if copied {
        "📸 Snapped! Copied to clipboard"
    } else {
        "📸 Snapped!"
    };
    let message = if notes.is_empty() {
        head.to_string()
    } else {
        format!("{head} ({})", notes.join("; "))
    };
    Ok(Snap { path, message })
}

impl Backend {
    fn settings(&self) -> Arc<Settings> {
        self.settings.read().unwrap().clone()
    }

    async fn apply_settings(&self, value: Option<Value>) {
        let (settings, err) = Settings::from_value(value);
        *self.settings.write().unwrap() = Arc::new(settings);
        if let Some(err) = err {
            self.client.show_message(MessageType::WARNING, err).await;
        }
    }

    async fn run_snap(&self, args: SnapArgs) -> Result<Snap> {
        let (code, lang) = {
            let docs = self.docs.read().unwrap();
            let doc = docs.get(&args.uri).context("document not open")?;
            (
                slice::selection(&doc.text, args.range),
                doc.language_id.clone(),
            )
        };
        if code.trim().is_empty() {
            anyhow::bail!("selection is empty");
        }
        let mut settings = (*self.settings()).clone();
        if let Some(ln) = args.line_numbers {
            settings.render.line_numbers = ln;
        }
        let file_name = args
            .uri
            .to_file_path()
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
            .or_else(|| {
                args.uri
                    .path_segments()
                    .and_then(|mut s| s.next_back())
                    .map(str::to_string)
            });
        let start_line = args.range.start.line as usize + 1;
        let copy = settings.copy_to_clipboard;
        tokio::task::spawn_blocking(move || {
            snap(
                &code,
                Some(&lang),
                file_name.as_deref(),
                &settings,
                None,
                copy,
                Some(start_line),
            )
        })
        .await?
    }
}

fn snap_action(
    title: &str,
    uri: &Url,
    range: Range,
    line_numbers: Option<bool>,
) -> CodeActionOrCommand {
    let mut args = serde_json::json!({ "uri": uri, "range": range });
    if let Some(ln) = line_numbers {
        args["line_numbers"] = ln.into();
    }
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::new("refactor.snap")),
        command: Some(Command {
            title: title.to_string(),
            command: SNAP_COMMAND.to_string(),
            arguments: Some(vec![args]),
        }),
        ..Default::default()
    })
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> jsonrpc::Result<InitializeResult> {
        self.apply_settings(params.initialization_options).await;
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "snapcode".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![CodeActionKind::new("refactor.snap")]),
                        ..Default::default()
                    },
                )),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![SNAP_COMMAND.to_string()],
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Ok(())
    }

    async fn did_open(&self, p: DidOpenTextDocumentParams) {
        let doc = Doc {
            text: p.text_document.text,
            language_id: p.text_document.language_id,
        };
        self.docs.write().unwrap().insert(p.text_document.uri, doc);
    }

    async fn did_change(&self, p: DidChangeTextDocumentParams) {
        // Full sync: the last change holds the whole document.
        if let Some(change) = p.content_changes.into_iter().last() {
            if let Some(doc) = self.docs.write().unwrap().get_mut(&p.text_document.uri) {
                doc.text = change.text;
            }
        }
    }

    async fn did_close(&self, p: DidCloseTextDocumentParams) {
        self.docs.write().unwrap().remove(&p.text_document.uri);
    }

    async fn did_change_configuration(&self, p: DidChangeConfigurationParams) {
        let has_values = p.settings.as_object().is_some_and(|o| !o.is_empty());
        if has_values {
            self.apply_settings(Some(p.settings)).await;
        }
    }

    async fn code_action(
        &self,
        p: CodeActionParams,
    ) -> jsonrpc::Result<Option<CodeActionResponse>> {
        if p.range.start == p.range.end {
            return Ok(None);
        }
        let uri = &p.text_document.uri;
        let numbers = self.settings().render.line_numbers;
        Ok(Some(vec![
            snap_action("📸 Snap selection", uri, p.range, None),
            snap_action(
                if numbers {
                    "📸 Snap selection (no line numbers)"
                } else {
                    "📸 Snap selection (with line numbers)"
                },
                uri,
                p.range,
                Some(!numbers),
            ),
        ]))
    }

    async fn execute_command(&self, p: ExecuteCommandParams) -> jsonrpc::Result<Option<Value>> {
        if p.command != SNAP_COMMAND {
            return Ok(None);
        }
        let args = p
            .arguments
            .into_iter()
            .next()
            .and_then(|v| serde_json::from_value::<SnapArgs>(v).ok());
        let Some(args) = args else {
            self.client
                .show_message(MessageType::ERROR, "snapcode: bad command arguments")
                .await;
            return Ok(None);
        };
        match self.run_snap(args).await {
            Ok(snap) => {
                self.client
                    .show_message(MessageType::INFO, &snap.message)
                    .await;
                if let (true, Some(path)) = (self.settings().open_after_snap, snap.path) {
                    if let Ok(uri) = Url::from_file_path(&path) {
                        let _ = self
                            .client
                            .show_document(ShowDocumentParams {
                                uri,
                                external: Some(false),
                                take_focus: Some(true),
                                selection: None,
                            })
                            .await;
                    }
                }
            }
            Err(e) => {
                self.client
                    .show_message(MessageType::ERROR, format!("snapcode: {e:#}"))
                    .await;
            }
        }
        Ok(None)
    }
}

pub fn run() -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let (service, socket) = LspService::new(|client| Backend {
            client,
            docs: RwLock::new(HashMap::new()),
            settings: RwLock::new(Arc::new(Settings::default())),
        });
        Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
            .serve(service)
            .await;
    });
    // The blocking stdin reader never finishes on its own; don't wait for it.
    rt.shutdown_background();
    Ok(())
}
