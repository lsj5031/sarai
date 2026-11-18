# Sarai

**Sarai** (攫い, from 攫う *sarau* — to snatch, carry off) points your coding
agents at **your** provider and models. It rewrites the config files each agent
already reads, so no gateway, account, plugin or runtime sits in the middle —
and it can put every original value back.

One small definition file (TOML, YAML or JSON) holds your endpoint, key and
model catalog. Sarai writes it into the agents you choose, preserving comments,
key order, indentation and line endings everywhere it does not own.

## Quick start

Extract the archive for your OS and architecture into a writable directory.
**No Python, Rust, package installation or network access is needed to run
Sarai.** Copy the template to a working definition, put your provider and key
in the copy, then run from the extracted folder:

```powershell
Copy-Item defs/example.toml defs/my-provider.toml
.\sarai.exe --list
.\sarai.exe -d defs/my-provider.toml --dry-run
.\sarai.exe -d defs/my-provider.toml
.\sarai.exe -d defs/my-provider.toml --restore
```

On Linux and macOS use `cp defs/example.toml defs/my-provider.toml` and run
`./sarai`.

`defs/my-provider.toml` is git-ignored, so a real key stays out of the
repository. `defs/example.toml` stays a placeholder-only template; do not put
a live key in a tracked file.

Keep the same definition, agent home and `.state` directory for restore. The
CLI stores originals in `.state/stash.json` **beside the executable**, not in
the current directory. Pass `--state-dir PATH` on both apply and restore to
choose another location. Protect that directory and your definition: both may
contain API keys, so never commit a definition that holds a real key. Sarai
configures agents; it does not install them or provide a gateway.

## The definition

`defs/example.toml` is a real TOML file. Copy it to `defs/my-provider.toml`
for your own values. YAML and JSON are also accepted.

```toml
[provider]
id = "mygw"                 # name written into each agent's config

[gateway]
base_url = "http://127.0.0.1:3425/v1"
api_key  = "sk-your-gateway-key"

[[models]]
id = "deepseek/deepseek-v4-pro"
name = "DeepSeek V4 Pro"
context = 200000
output = 16384
reasoning = true
efforts = ["low", "medium", "high"]
default_effort = "medium"
apis = ["chat", "responses"]

[agents]
pi = "deepseek/deepseek-v4-pro"
droid = "deepseek/deepseek-v4-pro"
codex = "deepseek/deepseek-v4-pro"
opencode = "deepseek/deepseek-v4-pro"
```

`gateway.base_url` is your Chat Completions / Responses endpoint. Sarai derives
the `/v1` URL and the root URL (without `/v1`) for Anthropic Messages. Every
`[agents]` entry must name a model from `[[models]]`; a typo fails the run
instead of being silently skipped.

Responses API is supported: Codex is always wired with `wire_api = "responses"`,
and any model with `"responses"` in its `apis` is given Droid's `openai`
provider instead of Chat Completions.

## Supported agents

| Agent id | Files written | What is injected |
|---|---|---|
| `pi` (aliases `omo`, `mu`, `stepcode`, `prime-agent`) | `~/.pi/agent/settings.json`, `models.json` in each agent's directory | `providers.<id>` block + model catalog; `defaultProvider`/`defaultModel`, `enabledModels`, thinking level |
| `droid` | `~/.factory/settings.json` | BYOK `customModels` entries + `sessionDefaultSettings.model` |
| `codex` | `$CODEX_HOME/config.toml` (default `~/.codex/config.toml`), `sarai-models.json` | `[model_providers.<id>]`, `model_provider`, `model`, `model_reasoning_effort` (from `default_effort`), `model_catalog_json` |
| `opencode` (aliases `mimocode`, `deveco`) | `$XDG_CONFIG_HOME/<id>/<id>.json(c)` | v1 `provider.<id>` or v2 `providers.<id>` block; `model`, `small_model` |

Aliases reuse the driver but target their own config directories.

Codex uses the selected model's `default_effort` if it is among the model's
supported levels. Otherwise it uses the lowest supported level, or clears
`model_reasoning_effort` if there are none. `--restore` puts the original
setting back.

## Restore and dry runs

Writes are idempotent: a second apply changes no bytes. `--dry-run` writes
nothing and creates no directories, including with `--restore`.

`--restore` puts back the stashed values and removes files Sarai created. It
only acts on agents with a stash entry in the selected state directory, so
restoring twice, or restoring after `--no-stash`, is a no-op rather than a
deletion.

## Build from source

With a current stable Rust toolchain:

```bash
cargo build --release --locked
```

This produces `target/release/sarai` (`sarai.exe` on Windows). On Windows,
`./scripts/package.ps1` builds the host-target ZIP into `dist/`; pass
`-Offline` when Cargo's dependencies are already cached. Build separately for
each target platform; the Windows MSVC build links the C runtime statically.

## Tests

```bash
cargo test --locked
```

The unit tests cover the JSONC and TOML editors, including byte-preserving
edits, CRLF/BOM handling, trailing commas and array creation. The end-to-end
suite in `tests/integration.rs` runs the compiled binary against isolated
temporary homes and checks apply, idempotence, dry runs, restore and the
model-switch behavior. No real agent configuration is touched.

CI builds and tests the CLI on Linux x86_64, macOS arm64 and Windows x86_64
MSVC, packages each executable and smoke-tests the extracted build.

## Proof with the real CLIs

`tests/proof.rs` gives Codex and Droid isolated homes, injects a definition
pointing at a fake OpenAI-compatible gateway (served in-process on
`127.0.0.1:18765`), and runs both CLIs non-interactively. It requires `codex`
and `droid` on `PATH` and is ignored by default:

```bash
cargo build --release
cargo test --release --test proof -- --ignored --nocapture
```

A pass means `codex exec` and `droid exec` both finished, printed `PROOF-OK`,
and the gateway received `POST /v1/responses` and `POST /v1/chat/completions`
with your API key and model id.

## How it works

The implementation lives in `src/`:

- `main.rs` — CLI dispatch, atomic writes, the restore stash.
- `definition.rs` — reads and validates TOML/YAML/JSON definitions.
- `jsonfile.rs` — JSONC editor. Parses just enough to find exact value spans,
  then splices text, so comments, key order, indentation and CRLF survive.
- `json_agents.rs` — the Pi, Droid and OpenCode families.
- `codex.rs` — Codex's TOML via `toml_edit`, plus its model catalog.
- `assets/codex_prompt.md` — the instruction text embedded in each catalog
  entry.

Adding a Pi- or OpenCode-family alias is a registration line plus its directory
default; a new config format gets a small module and a dispatch arm.

## Limitations

- Pi-family aliases use the shared shape, but per-agent quirks (Pi's extension
  and protocol fields, mu/prime's gateway keys) are not ported yet.
- OpenCode v2 migration is detected from `providers` in the file or
  `opencode_version = "2"` in that agent's table, not by probing the binary.
- The Codex catalog carries Codex's own bundled instruction prompt
  (`assets/codex_prompt.md`, originally openai/codex Apache-2.0) because Codex
  rejects entries without it.
- Comments inside a block Sarai replaces (a provider block, the `customModels`
  array) are not carried over; comments elsewhere, key order, indentation and
  CRLF line endings are preserved.
- `--restore` only acts on agents that have a stash entry; if the stash is gone
  it skips instead of guessing.
- No YAML round-trip editor yet, which blocks shapes such as Goose, Hermes and
  Raycast.

## Roadmap

The remaining work is mostly more shapes over the same three operations: read a
config, put a provider block in, set a model field.

- Shared shapes next: the OpenCode-like family (including `openchamber`), the
  Pi-like family (including `stepcode`), Codex profiles, `crush`, `goose` and
  `gemini` — roughly 12–15 agents for about six shapes.
- Then the bespoke "one provider block + one model field" JSON/TOML agents,
  around 45 more.
- Out of scope for a pure file injector: agents that need a running app's
  API/socket, a keychain/credential store, SQLite writes, MCP/file-copy logic,
  shell-profile functions, deeplinks or a JS plugin runtime.

## Attribution

Config shapes and preservation rules were learned from
[`yetone/magpie`](https://github.com/yetone/magpie) (MIT). No magpie code is
vendored except the Codex instruction text noted above, which originates in
[`openai/codex`](https://github.com/openai/codex) (Apache-2.0).

## License

Sarai is released under the [MIT license](LICENSE). The Codex instruction
prompt in `assets/codex_prompt.md` remains Apache-2.0; see
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
