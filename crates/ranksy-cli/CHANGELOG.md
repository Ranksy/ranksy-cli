# Changelog

## [0.11.1](https://github.com/Ranksy/ranksy-cli/compare/v0.11.0...v0.11.1) (2026-11-07)


### Bug Fixes

* `ranksy ads connect` lists all your Ranksy apps, not just the first 1,900. ([263d97c](https://github.com/Ranksy/ranksy-cli/commit/263d97ccb8eecfa168f95ed16dafa6b9bd5f1da8))
* `ranksy ads connect` no longer undoes a `disconnect` or `connect` made while it waited for the Partners login. ([263d97c](https://github.com/Ranksy/ranksy-cli/commit/263d97ccb8eecfa168f95ed16dafa6b9bd5f1da8))
* `ranksy ads sync --resume` only reuses days pulled at least 48 hours after they ended, so it no longer overwrites complete ads numbers with partial ones. If you used `--resume` before, run a normal `ranksy ads sync --from <first day you resumed> --to <today>` once to correct days it may have overwritten. ([263d97c](https://github.com/Ranksy/ranksy-cli/commit/263d97ccb8eecfa168f95ed16dafa6b9bd5f1da8))
* a Partners login to a different organization now gives a clear error when the wanted organization can't be opened. ([263d97c](https://github.com/Ranksy/ranksy-cli/commit/263d97ccb8eecfa168f95ed16dafa6b9bd5f1da8))
* the config file is written atomically with 0600 permissions, and a config file that can't be parsed is reported instead of showing "no API key found". ([263d97c](https://github.com/Ranksy/ranksy-cli/commit/263d97ccb8eecfa168f95ed16dafa6b9bd5f1da8))
* two `ranksy ads` commands no longer share one Chrome; the second waits for the first to finish. ([263d97c](https://github.com/Ranksy/ranksy-cli/commit/263d97ccb8eecfa168f95ed16dafa6b9bd5f1da8))

## [0.11.0](https://github.com/Ranksy/ranksy-cli/compare/v0.10.0...v0.11.0) (2026-10-27)


### Features

* ranksy ads sync pulls today by default and always refreshes it ([#26](https://github.com/Ranksy/ranksy-cli/issues/26)) ([27e6da7](https://github.com/Ranksy/ranksy-cli/commit/27e6da79d7e6879deeb89dc7edb3d8f1c7bf2c23))
* read Shopify Ads performance with ranksy ads overview, trend, keywords and search-terms ([#24](https://github.com/Ranksy/ranksy-cli/issues/24)) ([0813a89](https://github.com/Ranksy/ranksy-cli/commit/0813a8976dfdb0300fd10abe9f32671bbc3bfd52))
* show ad relevance and suggested bid range in ranksy ads keywords ([#25](https://github.com/Ranksy/ranksy-cli/issues/25)) ([b1440b1](https://github.com/Ranksy/ranksy-cli/commit/b1440b145dac03c8add2ff4b5870772c90de45ed))

## [0.10.0](https://github.com/Ranksy/ranksy-cli/compare/v0.9.1...v0.10.0) (2026-10-18)


### Features

* quit the Chrome that ranksy ads sync started, and sync headless ([#21](https://github.com/Ranksy/ranksy-cli/issues/21)) ([3aba823](https://github.com/Ranksy/ranksy-cli/commit/3aba823212dda26954564f87136fc763d1210990))

## [0.9.1](https://github.com/Ranksy/ranksy-cli/compare/v0.9.0...v0.9.1) (2026-10-17)


### Bug Fixes

* say what ranksy ads sync --only-app still pulls ([#19](https://github.com/Ranksy/ranksy-cli/issues/19)) ([1a23aac](https://github.com/Ranksy/ranksy-cli/commit/1a23aac8da9cf0884da44d06987870d73f97f67c))

## [0.9.0](https://github.com/Ranksy/ranksy-cli/compare/v0.8.0...v0.9.0) (2026-10-17)


### Features

* generate the command reference with `ranksy docs markdown` ([#16](https://github.com/Ranksy/ranksy-cli/issues/16)) ([50414f7](https://github.com/Ranksy/ranksy-cli/commit/50414f77b88c3d7889c25212429f6e294ba3ebed))
* sync one app with ranksy ads sync --only-app ([#18](https://github.com/Ranksy/ranksy-cli/issues/18)) ([040eb2d](https://github.com/Ranksy/ranksy-cli/commit/040eb2d2e197d1b2371abe769194e4795a581a0c))

## [0.8.0](https://github.com/Ranksy/ranksy-cli/compare/v0.7.0...v0.8.0) (2026-10-17)


### Features

* sync App Store ads from several Partners organizations ([#14](https://github.com/Ranksy/ranksy-cli/issues/14)) ([1c1736b](https://github.com/Ranksy/ranksy-cli/commit/1c1736b9fa0714f0822a69998bae2e2b077fb1df))

## [0.7.0](https://github.com/Ranksy/ranksy-cli/compare/v0.6.0...v0.7.0) (2026-10-16)


### Features

* add --after and --before to `ranksy ads sync` ([#11](https://github.com/Ranksy/ranksy-cli/issues/11)) ([75bc494](https://github.com/Ranksy/ranksy-cli/commit/75bc494a3308d54f1aeb26b04e9abbeef4397019))
* send Shopify's keyword recommendations with the ads snapshot ([#12](https://github.com/Ranksy/ranksy-cli/issues/12)) ([11d75b8](https://github.com/Ranksy/ranksy-cli/commit/11d75b859107b5e6d190133594283ce76d9d7dce))

## [0.6.0](https://github.com/Ranksy/ranksy-cli/compare/v0.5.0...v0.6.0) (2026-10-16)


### Features

* add `ranksy ads connect` and `ranksy ads sync` for Shopify App Store ads ([#9](https://github.com/Ranksy/ranksy-cli/issues/9)) ([194887e](https://github.com/Ranksy/ranksy-cli/commit/194887edd17bef47b932606e63b88133bd807956))

## [0.5.0](https://github.com/Ranksy/ranksy-cli/compare/v0.4.0...v0.5.0) (2026-09-04)


### Features

* keyword track/untrack + listing commands, cwd .env loading ([#7](https://github.com/Ranksy/ranksy-cli/issues/7)) ([2e3320d](https://github.com/Ranksy/ranksy-cli/commit/2e3320d7a71a13d376f012ab48b782605cb26151))

## [0.4.0](https://github.com/Ranksy/ranksy-cli/compare/v0.3.0...v0.4.0) (2026-08-29)


### Features

* MCP-parity analytics commands ([#5](https://github.com/Ranksy/ranksy-cli/issues/5)) ([db09e95](https://github.com/Ranksy/ranksy-cli/commit/db09e950bd4eb189e2428533f2ef246d73d84dbd))

## [0.2.0](https://github.com/Ranksy/ranksy-cli/compare/v0.1.0...v0.2.0) (2026-08-28)


### Features

* CLI dispatch, global flags, login/whoami, exit codes ([34b1cd5](https://github.com/Ranksy/ranksy-cli/commit/34b1cd509929c1c1113e2c89834e54954b45fda3))
* config loading and API key resolution precedence ([8da94eb](https://github.com/Ranksy/ranksy-cli/commit/8da94eb3f3e360a9fd5c5c64c7e896f38c71f10f))
* ranksy CLI — scriptable alternative to the MCP server ([10b186e](https://github.com/Ranksy/ranksy-cli/commit/10b186eef9ae62142c5001f26a6b1813fd6a0c28))
* scaffold ranksy-cli workspace and binary ([3a3616d](https://github.com/Ranksy/ranksy-cli/commit/3a3616d4c3b06b38378282ec953d951f01221689))
* table/json output rendering with color and tty awareness ([1f94e3f](https://github.com/Ranksy/ranksy-cli/commit/1f94e3fab87409c26432ee07c4b47321af287ca8))
