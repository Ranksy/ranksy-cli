# ranksy

`ranksy` is a CLI for running automations against the Ranksy API from your
terminal and CI. I built it as a scriptable alternative to the Ranksy MCP
server: every command maps to an API endpoint and prints a table or JSON, so it
fits bash, cron, and pipelines.

**Full reference: [api.ranksyapp.com/cli](https://api.ranksyapp.com/cli)**:
every command with its flags, API endpoint, and scope, plus the App Store ads
sync guide.

## Install

```bash
curl -LsSf https://ranksyapp.com/cli-installer.sh | sh
```

Prebuilt binaries for Linux, macOS, and Windows are on
[GitHub Releases](https://github.com/ranksy/ranksy-cli/releases). Update with
`ranksy update`.

## Log in

Use an API key from **Settings** > **API Keys** in Ranksy:

```bash
ranksy login rk_live_xxx
# or, for CI:
export RANKSY_API_KEY=rk_live_xxx
```

## Example

```bash
ranksy apps list
ranksy --app 01HZX9ABCDEF keywords list --json | jq '.data[0].organic_rank'
```

## App Store ads

`ranksy ads sync` pulls Shopify Partners ads into Ranksy for every mapped app. To
backfill just one app, add `--only-app` (Ranksy app id, Shopify app id, or app
name; repeat it for several): `ranksy ads sync --only-app "My App" --from 2025-11-01 --to 2025-12-20`.
The daily campaign list is still pulled for the whole organization, but other
apps' breakdowns, search terms and keywords are skipped, and nothing is pushed
for them.

Read the synced ads back with `ads overview`, `ads trend`, `ads keywords` and
`ads search-terms` (key scope `ads:read`; revenue and ROAS also need
`metrics:read`). All four take `--start-date`, `--end-date`, `--campaign`
(Shopify campaign id or name) and `--ad-type`; the period defaults to the 30
days up to yesterday:

```bash
ranksy --app 01HZX9ABCDEF ads overview --compare
ranksy --app 01HZX9ABCDEF ads keywords --campaign "Search - core" --sort-by cpi
ranksy --app 01HZX9ABCDEF ads search-terms --flag converting --json
```

## Development

```bash
# workspace: ranksy-api (typed client + auth wrapper) and ranksy-cli (binary)
cargo test

# re-sync the API client after the app's Scramble export changes:
# 1. copy the fresh openapi.json into the repo root
# 2. python3 scripts/normalize_openapi.py   (3.1 -> 3.0 normalization, recorded)
# 3. cargo build                             (progenitor regenerates the client)

# regenerate the command reference for api.ranksyapp.com/cli/commands
# (in Ranksy/api-docs: RANKSY_BIN=path/to/ranksy npm run sync:cli)
cargo run -q -p ranksy-cli -- docs markdown
```

When you add or change a command, add it to `REFERENCE` in
`crates/ranksy-cli/src/docs.rs` with its endpoint and scope. A test fails until
you do.
