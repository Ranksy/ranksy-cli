# ranksy

`ranksy` is a CLI for running automations against the Ranksy API from your
terminal and CI. I built it as a scriptable alternative to the Ranksy MCP
server: every command maps to an API operation and speaks table or JSON, so it
fits bash, cron, and pipelines.

It is stateless — no local workflow engine, no hidden state beyond the config
file it writes for `ranksy login`.

## Install

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ranksy/ranksy-cli/releases/latest/download/ranksy-cli-installer.sh | sh
```

Prebuilt static binaries for Linux (musl), macOS, and Windows are published to
GitHub Releases.

## Authenticate

Use the same API key you issue for the Ranksy API v1 in the dashboard
(`rk_live_...` for live data, `rk_test_...` for test data). No separate key is
needed.

```bash
# interactive convenience: writes ~/.config/ranksy/config.toml (0600)
ranksy login rk_live_xxx

# or, for CI, the env var route:
export RANKSY_API_KEY=rk_live_xxx
```

Key resolution, highest priority first: `--api-key` flag, `RANKSY_API_KEY`
env var, `~/.config/ranksy/config.toml`.

## Usage

```bash
ranksy apps list
ranksy --app 01HZX9ABCDEF rankings get
ranksy --app 01HZX9ABCDEF keywords list
ranksy --app 01HZX9ABCDEF reviews list
ranksy --app 01HZX9ABCDEF installs get
```

Pipeline example — pipe JSON into `jq`:

```bash
ranksy --app 01HZX9ABCDEF keywords list --json | jq '.data[0].organic_rank'
```

Global flags:

- `--json` — raw JSON instead of a table
- `--api-key <key>` — override the resolved key
- `--base-url <url>` — override the API base URL (default `https://ranksyapp.com/api/v1`)
- `--app <id>` — default app, falls back to config
- `--watch <secs>` — re-run the command every N seconds
- `-q` / `--quiet` — suppress non-essential output

Exit codes: `0` success, `2` usage error, `1` API or runtime failure — so CI
can gate on the result.

## Known gaps

These commands exist in the surface but have no API v1 endpoint yet, so they
fail with a clear "not implemented" error (exit 1) instead of pretending:

- `keywords track` / `keywords untrack`
- `listing get`

## Development

```bash
# workspace: ranksy-api (typed client + auth wrapper) and ranksy-cli (binary)
cargo test

# re-sync the API client after the app's Scramble export changes:
# 1. copy the fresh openapi.json into the repo root
# 2. python3 scripts/normalize_openapi.py   (3.1 -> 3.0 normalization, recorded)
# 3. cargo build                             (progenitor regenerates the client)
```
