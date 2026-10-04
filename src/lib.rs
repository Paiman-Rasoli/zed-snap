use std::fs;

use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

const REPO: &str = "Paiman-Rasoli/zed-snap";
const BINARY: &str = "snapcode-lsp";
const SERVER_ID: &str = "snapcode";

struct ZedSnap {
    cached_binary: Option<String>,
}

impl ZedSnap {
    fn binary_path(&mut self, id: &LanguageServerId, worktree: &zed::Worktree) -> Result<String> {
        if let Some(path) = LspSettings::for_worktree(SERVER_ID, worktree)
            .ok()
            .and_then(|s| s.binary)
            .and_then(|b| b.path)
        {
            return Ok(path);
        }
        if let Some(path) = worktree.which(BINARY) {
            return Ok(path);
        }
        if let Some(path) = &self.cached_binary {
            if fs::metadata(path).is_ok_and(|m| m.is_file()) {
                return Ok(path.clone());
            }
        }

        zed::set_language_server_installation_status(
            id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let release = zed::latest_github_release(
            REPO,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        let (os, arch) = zed::current_platform();
        let target = match (os, arch) {
            (zed::Os::Mac, zed::Architecture::Aarch64) => "aarch64-apple-darwin",
            (zed::Os::Mac, _) => "x86_64-apple-darwin",
            (zed::Os::Linux, zed::Architecture::Aarch64) => "aarch64-unknown-linux-gnu",
            (zed::Os::Linux, _) => "x86_64-unknown-linux-gnu",
            (zed::Os::Windows, zed::Architecture::Aarch64) => "aarch64-pc-windows-msvc",
            (zed::Os::Windows, _) => "x86_64-pc-windows-msvc",
        };
        let (ext, file_type, exe) = match os {
            zed::Os::Windows => ("zip", zed::DownloadedFileType::Zip, ".exe"),
            _ => ("tar.gz", zed::DownloadedFileType::GzipTar, ""),
        };
        let asset_name = format!("{BINARY}-{}-{target}.{ext}", release.version);
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| format!("no release asset named {asset_name}"))?;

        let dir = format!("{BINARY}-{}", release.version);
        let binary = format!("{dir}/{BINARY}{exe}");
        if !fs::metadata(&binary).is_ok_and(|m| m.is_file()) {
            zed::set_language_server_installation_status(
                id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            zed::download_file(&asset.download_url, &dir, file_type)
                .map_err(|e| format!("failed to download {asset_name}: {e}"))?;
            zed::make_file_executable(&binary)?;
            // Remove older versions.
            if let Ok(entries) = fs::read_dir(".") {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with(BINARY) && name != dir {
                        fs::remove_dir_all(entry.path()).ok();
                    }
                }
            }
        }
        zed::set_language_server_installation_status(
            id,
            &zed::LanguageServerInstallationStatus::None,
        );
        self.cached_binary = Some(binary.clone());
        Ok(binary)
    }

    fn user_options(worktree: &zed::Worktree) -> Option<zed::serde_json::Value> {
        LspSettings::for_worktree(SERVER_ID, worktree)
            .ok()
            .and_then(|s| s.initialization_options)
    }
}

impl zed::Extension for ZedSnap {
    fn new() -> Self {
        Self {
            cached_binary: None,
        }
    }

    fn language_server_command(
        &mut self,
        id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let args = LspSettings::for_worktree(SERVER_ID, worktree)
            .ok()
            .and_then(|s| s.binary)
            .and_then(|b| b.arguments)
            .unwrap_or_else(|| vec!["lsp".into()]);
        Ok(zed::Command {
            command: self.binary_path(id, worktree)?,
            args,
            env: worktree.shell_env(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        Ok(Self::user_options(worktree))
    }

    fn language_server_workspace_configuration(
        &mut self,
        _id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        Ok(Self::user_options(worktree))
    }
}

zed::register_extension!(ZedSnap);
