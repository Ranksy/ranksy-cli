# ranksy

`ranksy` is a CLI for running automations against the Ranksy API from your
terminal and CI. I built it as a scriptable alternative to the Ranksy MCP
server: every command maps to an API operation and speaks table or JSON, so it
fits bash, cron, and pipelines.

It is stateless — no local workflow engine, no hidden state beyond the config
file it writes for `ranksy login`.

## Install

```bash
curl -LsSf https://ranksyapp.com/cli-installer.sh | sh
```

Prebuilt static binaries for Linux (musl), macOS, and Windows are published to
GitHub Releases.

### Update

```bash
ranksy update          # install the latest release
ranksy update --check  # report whether a newer version exists, without installing
```

Self-update works for installer-based installs (it reads the receipt the
installer writes); if you installed another way, re-run the install command
above. `ranksy --version` prints the current version.

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

`ranksy` also loads a `.env` from the current directory (walking up parent
dirs) at startup, so `RANKSY_API_KEY` (and `RANKSY_BASE_URL`) in a project-local
`.env` are picked up without exporting them. A variable already exported in your
shell wins over the `.env` file.

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

## Managing keywords

```bash
ranksy --app 01HZX9ABCDEF keywords track "email marketing"   # needs keywords:write
ranksy --app 01HZX9ABCDEF keywords untrack "email marketing" # text or slug
```

`untrack` slugifies the keyword to match the tracked row, so both the keyword
text and its slug work. If a keyword's slug collided on track (a rare sha1
suffix), untrack it with the exact `slug` shown in `keywords list`.

## Development

```bash
# workspace: ranksy-api (typed client + auth wrapper) and ranksy-cli (binary)
cargo test

# re-sync the API client after the app's Scramble export changes:
# 1. copy the fresh openapi.json into the repo root
# 2. python3 scripts/normalize_openapi.py   (3.1 -> 3.0 normalization, recorded)
# 3. cargo build                             (progenitor regenerates the client)
```
