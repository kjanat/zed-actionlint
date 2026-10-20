# actionlint for Zed

GitHub Actions diagnostics, expression completions, and quick fixes from
[`kjanat/actionlint`](https://github.com/kjanat/actionlint)'s language server.

## Requirements

- A current Zed release supporting extension API 0.7.0.
- Zed's [GitHub Actions extension](https://zed.dev/extensions/github-actions),
  which defines the **GitHub Actions** language and syntax highlighting.
- An actionlint build that supports `actionlint lsp`. Verify with
  `actionlint lsp --help`. As of October 20, 2026, the latest published release,
  v1.17.0, lacks LSP support. Build the current `kjanat/actionlint` source;
  `rhysd/actionlint` does not provide this server.
- For installing this extension from source: Rust installed through
  [rustup](https://rustup.rs). Check `rustup --version`.

The extension uses an existing actionlint installation. It does not download
release assets while the published binaries lack LSP support.

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

By default, the extension finds `actionlint` on the project's shell `PATH`,
checks `actionlint lsp --help`, and launches `actionlint lsp`.
Shell environment variables remain available to actionlint and optional tools
such as ShellCheck and Ruff.

Build from the actionlint source checkout with its required Go toolchain:

```sh
go build -o actionlint-lsp.exe ./cmd/actionlint
```

Then select that binary:

```json
{
  "lsp": {
    "actionlint": {
      "binary": {
        "path": "C:\\Users\\kjanat\\Projects\\actionlint\\actionlint-lsp.exe",
        "arguments": ["lsp"]
      }
    }
  }
}
```

On macOS or Linux, build with `-o actionlint-lsp` and use an absolute executable
path such as `/home/me/projects/actionlint/actionlint-lsp`.

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

The manifest grants only the subprocess arguments `lsp --help` for the
compatibility probe. It requests no download or npm-install capability.
