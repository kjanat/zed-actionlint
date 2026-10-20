use std::collections::BTreeMap;

use zed_extension_api::{self as zed, settings::LspSettings};

struct ActionlintExtension;

/// Explicit arguments replace the defaults, matching Zed's binary settings.
/// The shell environment keeps optional tools such as ShellCheck discoverable.
fn server_command(
    settings: Option<zed::settings::CommandSettings>,
    system_path: Option<String>,
    shell_env: zed::EnvVars,
) -> zed::Result<zed::Command> {
    let mut args = vec!["lsp".to_owned()];
    let mut env: BTreeMap<_, _> = shell_env.into_iter().collect();
    let mut path = system_path;

    if let Some(settings) = settings {
        if settings.path.is_some() {
            path = settings.path;
        }
        if let Some(arguments) = settings.arguments {
            args = arguments;
        }
        env.extend(settings.env.unwrap_or_default());
    }

    let command = path.filter(|path| !path.trim().is_empty()).ok_or_else(|| {
        "Install an LSP-capable build of kjanat/actionlint on PATH, or set \
         lsp.actionlint.binary.path and binary.arguments to [\"lsp\"] in Zed settings."
            .to_owned()
    })?;

    Ok(zed::Command {
        command,
        args,
        env: env.into_iter().collect(),
    })
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
    fn command(&self, worktree: &zed::Worktree) -> zed::Result<zed::Command> {
        let settings = LspSettings::for_worktree("actionlint", worktree)?;
        let command = server_command(
            settings.binary,
            worktree.which("actionlint"),
            worktree.shell_env(),
        )?;

        // Help exits immediately; never start a second language server to probe.
        let output = zed::process::Command::new(&command.command)
            .args(["lsp", "--help"])
            .envs(command.env.clone())
            .env("ACTIONLINT_JSON", "false")
            .env("ACTIONLINT_JSON_PRETTY", "false")
            .env("ACTIONLINT_COLOR", "never")
            .env("ACTIONLINT_HYPERLINKS", "never")
            .output()
            .map_err(|error| {
                format!(
                    "Could not check `{}` for LSP support: {error}",
                    command.command
                )
            })?;

        if !supports_lsp(&output) {
            return Err(format!(
                "`{}` does not provide `actionlint lsp`. Use an LSP-capable build of \
                 kjanat/actionlint. Released v1.17.0 and rhysd/actionlint do not provide this server.",
                command.command
            ));
        }

        Ok(command)
    }
}

impl zed::Extension for ActionlintExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let result = self.command(worktree);
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
}
