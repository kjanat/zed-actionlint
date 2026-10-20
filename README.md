# actionlint for Zed

GitHub Actions diagnostics, expression completions, and quick fixes from
[`kjanat/actionlint`](https://github.com/kjanat/actionlint)'s language server.

## Requirements

- A current Zed release supporting extension API 0.7.0.
- Zed's [GitHub Actions extension](https://zed.dev/extensions/github-actions),
  which defines the **GitHub Actions** language and syntax highlighting.
- For installing this extension from source: Rust installed through
  [rustup](https://rustup.rs). Check `rustup --version`.

The extension downloads actionlint using Zed's GitHub release API. An existing
LSP-capable installation on `PATH` takes precedence.

## Install

1. Install **GitHub Actions** from Zed's extension catalog.
2. Open Zed's Extensions page and click **Install Dev Extension**.
3. Select this repository's root. Zed compiles and installs the Rust extension.
4. Merge this into Zed's settings:

   ```json
   {
     "file_types": {
       "GitHub Actions": [
         "**/.github/workflows/*.yml",
         "**/.github/workflows/*.yaml"
       ]
     },
     "languages": {
       "GitHub Actions": {
         "language_servers": ["actionlint", "..."]
       }
     }
   }
   ```

The `...` preserves other servers registered for **GitHub Actions**, including
the language extension's server. Use `["actionlint"]` to run only actionlint.

Open a workflow and confirm the language picker shows **GitHub Actions**.
This extension deliberately does not register for **YAML**: actionlint's LSP
analyzes supplied documents as workflows or action metadata, so attaching it
to arbitrary YAML would produce unrelated diagnostics.

## Choose the binary

By default, the extension checks `actionlint` on the project's shell `PATH`.
If it is missing or does not support LSP, the extension fetches the latest stable
release from [`kjanat/actionlint`](https://github.com/kjanat/actionlint/releases)
through Zed's `latest_github_release` and `download_file` APIs. It selects the
native Windows ZIP or macOS/Linux tarball, extracts it into a versioned cache,
checks LSP support, and launches `actionlint lsp`.

The first managed launch after the extension loads checks GitHub for updates.
Successful downloads are reused for subsequent starts in the same extension
session. Reload the extension or restart Zed to check again. An existing archive's
executable is reused from disk when the selected release has not changed.
Older version directories remain intact so updates do not replace running servers.

Shell environment variables remain available to actionlint and optional tools
such as ShellCheck and Ruff.

To override automatic installation with your own executable:

```json
{
  "lsp": {
    "actionlint": {
      "binary": {
        "path": "C:\\tools\\actionlint.exe",
        "arguments": ["lsp"]
      }
    }
  }
}
```

On macOS or Linux, use an absolute executable path such as `/usr/local/bin/actionlint`.

**Include `arguments: ["lsp"]` when setting `path`.** Zed can launch explicit
binary paths directly, bypassing the extension's default arguments and
compatibility check. Verify that binary with `lsp --help` yourself.

Arguments replace the entire argument list. For example:

```json
{
  "lsp": {
    "actionlint": {
      "binary": {
        "arguments": ["lsp", "--config", "/path/to/actionlint.yaml"],
        "env": {
          "MY_TOOL_SETTING": "value"
        }
      }
    }
  }
}
```

Use actionlint's repository configuration or supported CLI flags for lint
settings. The current server does not consume LSP `initialization_options` or
workspace `settings`.

## Development

After editing Rust, use **Rebuild** on Zed's Extensions page. Check errors with
**zed: open log**. Zed builds the WebAssembly extension during dev installation.

Local source checks:

```sh
cargo fmt --check
cargo test --locked
```

To exercise the integration, open a workflow containing an unknown `needs` job,
confirm a diagnostic, correct the job name without saving, and confirm the
diagnostic clears. Check expression completions after `${{ github.`. Opening an
unrelated YAML file must not start actionlint for that language.

The manifest limits downloads to `kjanat/actionlint` release assets on GitHub,
and subprocess probes to the arguments `lsp --help`.
