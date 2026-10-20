use std::collections::BTreeMap;

use zed_extension_api::{self as zed, settings::LspSettings};

const REPOSITORY: &str = "kjanat/actionlint";

struct ActionlintExtension {
    cached_binary_path: Option<String>,
}

struct ReleaseBinary {
    asset_name: String,
    download_url: String,
    directory: String,
    executable: String,
    archive_type: zed::DownloadedFileType,
}

impl ReleaseBinary {
    fn new(
        release: &zed::GithubRelease,
        os: zed::Os,
        architecture: zed::Architecture,
    ) -> zed::Result<Self> {
        let version = release
            .version
            .strip_prefix('v')
            .unwrap_or(&release.version);
        if version.is_empty()
            || !version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-_".contains(&byte))
        {
            return Err(format!(
                "Unsupported actionlint release tag: {}",
                release.version
            ));
        }
        let platform = match os {
            zed::Os::Mac => "darwin",
            zed::Os::Linux => "linux",
            zed::Os::Windows => "windows",
        };
        let architecture = match architecture {
            zed::Architecture::Aarch64 => "arm64",
            zed::Architecture::X8664 => "amd64",
            zed::Architecture::X86 => "386",
        };
        let (extension, executable, archive_type) = match os {
            zed::Os::Windows => ("zip", "actionlint.exe", zed::DownloadedFileType::Zip),
            _ => ("tar.gz", "actionlint", zed::DownloadedFileType::GzipTar),
        };
        let asset_name = format!("actionlint_{version}_{platform}_{architecture}.{extension}");
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| {
                format!(
                    "actionlint {} has no release asset `{asset_name}`",
                    release.version
                )
            })?;
        Ok(Self {
            asset_name,
            download_url: asset.download_url.clone(),
            directory: format!("actionlint-{version}-{platform}-{architecture}"),
            executable: executable.to_owned(),
            archive_type,
        })
    }
}

fn server_environment(
    settings: Option<&zed::settings::CommandSettings>,
    shell_env: zed::EnvVars,
) -> zed::EnvVars {
    let mut env: BTreeMap<_, _> = shell_env.into_iter().collect();
    if let Some(overrides) = settings.and_then(|settings| settings.env.as_ref()) {
        env.extend(
            overrides
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    env.into_iter().collect()
}

/// Explicit arguments replace the defaults, matching Zed's binary settings.
/// The shell environment keeps optional tools such as ShellCheck discoverable.
fn server_command(
    settings: Option<zed::settings::CommandSettings>,
    system_path: Option<String>,
    shell_env: zed::EnvVars,
) -> zed::Result<zed::Command> {
    let mut args = vec!["lsp".to_owned()];
    let env = server_environment(settings.as_ref(), shell_env);
    let mut path = system_path;

    if let Some(settings) = settings {
        if settings.path.is_some() {
            path = settings.path;
        }
        if let Some(arguments) = settings.arguments {
            args = arguments;
        }
    }

    let command = path.filter(|path| !path.trim().is_empty()).ok_or_else(|| {
        "Set lsp.actionlint.binary.path to an executable and binary.arguments to [\"lsp\"], \
         or remove binary.path to download actionlint automatically from kjanat/actionlint."
            .to_owned()
    })?;

    Ok(zed::Command { command, args, env })
}

/// Old actionlint builds can print root help successfully without supporting LSP.
fn supports_lsp(output: &zed::process::Output) -> bool {
    output.status == Some(0)
        && [&output.stdout, &output.stderr].iter().any(|bytes| {
            let help = String::from_utf8_lossy(bytes);
            help.contains("Usage:") && help.contains("actionlint lsp")
        })
}

impl ActionlintExtension {
    fn cached_binary(&mut self) -> Option<String> {
        if let Some(path) = &self.cached_binary_path
            && std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
        {
            return Some(path.clone());
        }
        self.cached_binary_path = None;
        None
    }

    fn prepare_download(ready: &std::path::Path) -> zed::Result<()> {
        // A failed extraction may leave an executable behind. Invalidate the
        // previous success marker before replacing any files.
        match std::fs::remove_file(ready) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "Could not invalidate actionlint installation: {error}"
            )),
        }
    }

    fn check_binary(path: &str, env: &zed::EnvVars) -> zed::Result<bool> {
        // Help exits immediately; never start a second language server to probe.
        let output = zed::process::Command::new(path)
            .args(["lsp", "--help"])
            .envs(env.clone())
            .env("ACTIONLINT_JSON", "false")
            .env("ACTIONLINT_JSON_PRETTY", "false")
            .env("ACTIONLINT_COLOR", "never")
            .env("ACTIONLINT_HYPERLINKS", "never")
            .output()
            .map_err(|error| format!("Could not check `{path}` for LSP support: {error}"))?;
        Ok(supports_lsp(&output))
    }

    fn downloaded_binary(
        &mut self,
        id: &zed::LanguageServerId,
        env: &zed::EnvVars,
    ) -> zed::Result<String> {
        if let Some(path) = self.cached_binary() {
            return Ok(path);
        }

        zed::set_language_server_installation_status(
            id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let release = zed::latest_github_release(
            REPOSITORY,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;
        let (os, architecture) = zed::current_platform();
        let binary = ReleaseBinary::new(&release, os, architecture)?;
        // Zed's process API does not run probes in the extension's directory.
        let directory = std::env::current_dir()
            .map_err(|error| format!("Could not locate the extension directory: {error}"))?
            .join(&binary.directory);
        let path = directory
            .join(&binary.executable)
            .to_string_lossy()
            .into_owned();
        let ready = directory.join(".lsp-ready");

        if !ready.is_file() || !std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            Self::prepare_download(&ready)?;
            zed::set_language_server_installation_status(
                id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            zed::download_file(&binary.download_url, &binary.directory, binary.archive_type)
                .map_err(|error| format!("Could not download {}: {error}", binary.asset_name))?;
        }
        if !std::fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            return Err(format!(
                "{} did not contain {}",
                binary.asset_name, binary.executable
            ));
        }
        zed::make_file_executable(&path)?;
        if !Self::check_binary(&path, env)? {
            return Err(format!(
                "The latest {REPOSITORY} release ({}) does not provide `actionlint lsp`. \
                 An LSP-capable release must be published before automatic installation can start the server.",
                release.version
            ));
        }
        std::fs::write(&ready, release.version.as_bytes())
            .map_err(|error| format!("Could not record actionlint installation: {error}"))?;

        self.cached_binary_path = Some(path.clone());
        Ok(path)
    }

    fn command(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let settings = LspSettings::for_worktree("actionlint", worktree)?;
        let shell_env = worktree.shell_env();
        if settings
            .binary
            .as_ref()
            .is_some_and(|binary| binary.path.is_some())
        {
            // Current Zed launches explicit paths itself. Preserve that override
            // here too for hosts that delegate it to the extension.
            return server_command(settings.binary, None, shell_env);
        }
        let env = server_environment(settings.binary.as_ref(), shell_env.clone());
        let system_path = worktree
            .which("actionlint")
            .filter(|path| Self::check_binary(path, &env).unwrap_or(false));
        let path = match system_path {
            Some(path) => path,
            None => self.downloaded_binary(id, &env)?,
        };
        server_command(settings.binary, Some(path), shell_env)
    }
}

impl zed::Extension for ActionlintExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let result = self.command(id, worktree);
        let status = match &result {
            Ok(_) => zed::LanguageServerInstallationStatus::None,
            Err(error) => zed::LanguageServerInstallationStatus::Failed(error.clone()),
        };
        zed::set_language_server_installation_status(id, &status);
        result
    }
}

zed::register_extension!(ActionlintExtension);

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(json: &str) -> zed::settings::CommandSettings {
        zed::serde_json::from_str(json).expect("valid command settings")
    }

    #[test]
    fn launches_system_binary_over_stdio() {
        let command = server_command(
            None,
            Some("/usr/bin/actionlint".to_owned()),
            vec![("PATH".to_owned(), "/usr/bin".to_owned())],
        )
        .expect("system binary");
        assert_eq!(command.command, "/usr/bin/actionlint");
        assert_eq!(command.args, ["lsp"]);
        assert_eq!(command.env, [("PATH".to_owned(), "/usr/bin".to_owned())]);
    }

    #[test]
    fn overrides_path_and_arguments_without_losing_shell_environment() {
        let command = server_command(
            Some(settings(
                r#"{
                    "path": "C:\\tools\\actionlint.exe",
                    "arguments": ["lsp", "--config", "config with spaces.yml"],
                    "env": {"ACTIONLINT_CONFIG": "custom.yml"}
                }"#,
            )),
            Some("old-actionlint".to_owned()),
            vec![
                ("PATH".to_owned(), "tools".to_owned()),
                ("ACTIONLINT_CONFIG".to_owned(), "old.yml".to_owned()),
            ],
        )
        .expect("configured binary");
        assert_eq!(command.command, r"C:\tools\actionlint.exe");
        assert_eq!(command.args, ["lsp", "--config", "config with spaces.yml"]);
        assert_eq!(
            command.env,
            [
                ("ACTIONLINT_CONFIG".to_owned(), "custom.yml".to_owned()),
                ("PATH".to_owned(), "tools".to_owned())
            ]
        );
    }

    #[test]
    fn arguments_override_also_applies_to_system_binary() {
        let command = server_command(
            Some(settings(r#"{"arguments":["lsp","--no-shellcheck"]}"#)),
            Some("actionlint".to_owned()),
            vec![],
        )
        .expect("system binary with options");
        assert_eq!(command.args, ["lsp", "--no-shellcheck"]);
    }

    #[test]
    fn explicit_path_does_not_require_system_installation() {
        let command = server_command(
            Some(settings(r#"{"path":"/opt/actionlint"}"#)),
            None,
            vec![],
        )
        .expect("configured binary");
        assert_eq!(command.command, "/opt/actionlint");
        assert_eq!(command.args, ["lsp"]);
    }

    #[test]
    fn missing_and_empty_paths_have_setup_instructions() {
        for path in [None, Some(String::new()), Some(" ".to_owned())] {
            let error = server_command(None, path, vec![]).expect_err("missing executable");
            assert!(error.contains("binary.arguments"));
            assert!(error.contains("kjanat/actionlint"));
        }
    }

    #[test]
    fn checks_lsp_help_on_either_output_stream() {
        let help =
            b"Serve the language server over stdin/stdout\n\nUsage:\n  actionlint lsp [flags]\n";
        for (stdout, stderr) in [(help.to_vec(), vec![]), (vec![], help.to_vec())] {
            assert!(supports_lsp(&zed::process::Output {
                status: Some(0),
                stdout,
                stderr,
            }));
        }
    }

    #[test]
    fn rejects_legacy_help_and_failed_probes() {
        for (status, text) in [
            (Some(0), "Usage: actionlint [FLAGS] [FILES...]"),
            (Some(1), "could not read \"lsp\""),
            (Some(1), "actionlint lsp"),
            (None, "actionlint lsp"),
        ] {
            assert!(!supports_lsp(&zed::process::Output {
                status,
                stdout: text.as_bytes().to_vec(),
                stderr: vec![],
            }));
        }
    }

    #[test]
    fn selects_native_release_archives_and_executables() {
        for (os, architecture, asset_name, executable) in [
            (
                zed::Os::Mac,
                zed::Architecture::Aarch64,
                "actionlint_1.17.0_darwin_arm64.tar.gz",
                "actionlint",
            ),
            (
                zed::Os::Mac,
                zed::Architecture::X8664,
                "actionlint_1.17.0_darwin_amd64.tar.gz",
                "actionlint",
            ),
            (
                zed::Os::Linux,
                zed::Architecture::Aarch64,
                "actionlint_1.17.0_linux_arm64.tar.gz",
                "actionlint",
            ),
            (
                zed::Os::Linux,
                zed::Architecture::X8664,
                "actionlint_1.17.0_linux_amd64.tar.gz",
                "actionlint",
            ),
            (
                zed::Os::Linux,
                zed::Architecture::X86,
                "actionlint_1.17.0_linux_386.tar.gz",
                "actionlint",
            ),
            (
                zed::Os::Windows,
                zed::Architecture::Aarch64,
                "actionlint_1.17.0_windows_arm64.zip",
                "actionlint.exe",
            ),
            (
                zed::Os::Windows,
                zed::Architecture::X8664,
                "actionlint_1.17.0_windows_amd64.zip",
                "actionlint.exe",
            ),
            (
                zed::Os::Windows,
                zed::Architecture::X86,
                "actionlint_1.17.0_windows_386.zip",
                "actionlint.exe",
            ),
        ] {
            let url = format!(
                "https://github.com/kjanat/actionlint/releases/download/v1.17.0/{asset_name}"
            );
            let release = zed::GithubRelease {
                version: "v1.17.0".to_owned(),
                assets: vec![zed::GithubReleaseAsset {
                    name: asset_name.to_owned(),
                    download_url: url.clone(),
                }],
            };
            let binary =
                ReleaseBinary::new(&release, os, architecture).expect("matching release asset");
            assert_eq!(binary.download_url, url);
            assert_eq!(binary.executable, executable);
            match os {
                zed::Os::Windows => {
                    assert!(matches!(binary.archive_type, zed::DownloadedFileType::Zip))
                }
                _ => assert!(matches!(
                    binary.archive_type,
                    zed::DownloadedFileType::GzipTar
                )),
            }
            assert!(!binary.directory.contains('/'));
            assert!(!binary.directory.contains('\\'));
        }
    }

    #[test]
    fn rejects_missing_platform_asset_instead_of_running_another_platform() {
        let release = zed::GithubRelease {
            version: "v1.17.0".to_owned(),
            assets: vec![zed::GithubReleaseAsset {
                name: "actionlint_1.17.0_linux_amd64.tar.gz".to_owned(),
                download_url: "https://example.invalid/wrong-platform".to_owned(),
            }],
        };
        let result = ReleaseBinary::new(&release, zed::Os::Windows, zed::Architecture::X8664);
        assert!(result.is_err_and(|error| error.contains("actionlint_1.17.0_windows_amd64.zip")));
    }

    #[test]
    fn rejects_release_tags_that_could_escape_the_cache_directory() {
        for version in [
            "",
            "v",
            "../other",
            "v1/../../other",
            "v1\\other",
            "vC:other",
        ] {
            let release = zed::GithubRelease {
                version: version.to_owned(),
                assets: vec![],
            };
            let result = ReleaseBinary::new(&release, zed::Os::Linux, zed::Architecture::X8664);
            assert!(
                result.is_err_and(|error| error.contains("Unsupported actionlint release tag"))
            );
        }
    }

    #[test]
    fn failed_repair_cannot_reuse_a_partially_extracted_executable() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("current time")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "zed-actionlint-repair-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).expect("test directory");
        let binary = directory.join("actionlint");
        let ready = directory.join(".lsp-ready");
        std::fs::write(&binary, b"complete executable").expect("installed executable");
        std::fs::write(&ready, b"version").expect("installation marker");
        let mut extension = ActionlintExtension {
            cached_binary_path: Some(binary.to_string_lossy().into_owned()),
        };
        assert!(extension.cached_binary().is_some());

        std::fs::remove_file(&binary).expect("simulate missing executable");
        assert!(extension.cached_binary().is_none());
        ActionlintExtension::prepare_download(&ready).expect("start repair");
        std::fs::write(&binary, b"partial").expect("simulate interrupted extraction");
        assert!(extension.cached_binary().is_none());
        assert!(!ready.exists());
        ActionlintExtension::prepare_download(&ready).expect("retry interrupted repair");

        std::fs::remove_file(&binary).expect("remove test executable");
        std::fs::remove_dir(&directory).expect("remove test directory");
    }
}
