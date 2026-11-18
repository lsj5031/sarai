# agent-config-injector

A standalone, config-driven injector for coding agents. It rewrites each
agent's own config files so the agent uses **your** provider and models —
no magpie runtime, gateway, accounts or plugins involved.

The definition is yours (`defs/example.toml`); the code is a small, portable
core plus per-"shape" drivers. Magpie was used only as a reference for the
file formats and the preservation rules (MIT licence).

## Quick start

```powershell
python -m pip install -r requirements.txt
python -m injector -d defs/example.toml
python -m injector -d defs/example.toml --dry-run
python -m injector -d defs/example.toml --restore
python -m injector --list
```

The definition is a real TOML file (`defs/example.toml`); YAML/JSON are also
accepted. `provider.id`, `gateway.base_url`, `gateway.api_key` and `[[models]]`
are all yours. `base_url` is your Chat Completions / Responses endpoint; the
root URL (without `/v1`) is used for Anthropic Messages.

Responses API is supported: Codex is always wired with `wire_api = "responses"`,
and any model with `"responses"` in its `apis` is given Droid's `openai`
provider (Responses) instead of Chat Completions. The proof gateway serves
both `/v1/responses` and `/v1/chat/completions`.

## What is wired today

| Agent id | Files written | What is injected |
|---|---|---|
| `pi` (aliases `omo`, `mu`, `stepcode`, `prime-agent`) | `~/.pi/agent/settings.json`, `models.json` | `providers.<id>` block + model catalog; `defaultProvider`/`defaultModel`, `enabledModels`, thinking level |
| `droid` | `~/.factory/settings.json` | BYOK `customModels` entries + `sessionDefaultSettings.model` |
| `codex` | `~/.codex/config.toml`, `magpie-models.json` | `[model_providers.<id>]`, `model_provider`, `model`, `model_catalog_json` |
| `opencode` (aliases `mimocode`, `deveco`) | `~/.config/opencode/opencode.json(c)` | v1 `provider.<id>` or v2 `providers.<id>` block; `model`, `small_model` |

The aliases only reuse the shape; their real directories come from the
registry (`PI_DIRS`, `opencode.directory()`).

## Proof with the real CLIs

`proof/run_proof.py` gives Codex and Droid isolated homes, injects a definition
pointing at `proof/fake_gateway.py`, and runs both CLIs non-interactively.

Observed result (OpenAI Codex v0.169.0 and Factory Droid):

- `codex exec` → exit 0, prints `PROOF-OK`, shows `model: proof/echo-model`,
  `provider: mygw`; gateway logged `POST /v1/responses` with
  `Authorization: Bearer PROOF-KEY-123` and `model: proof/echo-model`.
- `droid exec` → exit 0, prints `PROOF-OK`; gateway logged
  `POST /v1/chat/completions` with the same key and model.

Run it yourself:

```powershell
python proof/fake_gateway.py        # terminal 1
python proof/run_proof.py           # terminal 2
```

One real fix came out of this: Codex's `model_catalog_json` must be an object
`{"models": [...]}`, not a bare array. That is now what `shapes/codex.py`
writes.

## How it works

- `definition.py` — loads your definition into a neutral `Definition`.
- `textjson.py` — JSON/JSONC editor. Parses just enough to find exact value
  spans, then splices text: comments, key order, indentation and CRLF survive.
- `tomlfile.py` — TOML editor backed by `tomlkit`, same preservation goal.
- `core.py` — `merge_preserve` (keys we don't own stay the user's; the model
  list is ours alone), `Stash` (originals for `--restore`), atomic writes.
- `shapes/` — one small module per config format.
- `registry.py` — agent id → shape driver. Adding an agent is one line here.

Writes are idempotent: a second apply changes no bytes. A `--dry-run` writes
nothing. `--restore` puts back the stashed values and removes files we made.

## Porting the rest of magpie: feasibility

I mapped Magpie's registry (`internal/agent/agents.go:314-408` `onMachineAll`)
and its 195 non-test Go files. Result:

- **91** builtin agent constructors, plus 4 dynamic profile families
  (`omp`, `qwenpaw`, `hermes`, `claude#…`), 40 WSL kinds and an unbounded
  plugin-defined class.
- Only **~18 of 91** are produced by a genuinely shared builder. The rest are
  bespoke constructors — but most still end in the same three operations:
  read config, put a provider block in, set a model field.
- **~60–65 of 91** are reducible to a standalone file injector if given
  `(gateway URL, agent key, model catalog, config dir)`.
- **~25–30 of 91** cannot be faithfully ported as a file injector alone:
  they need a running app's API/socket, an OS credential store/keychain,
  SQLite writes, MCP/file-copy logic, shell-profile functions, deeplinks,
  or the JS plugin runtime.

### The families (biggest wins first)

| Shape / builder | Agents covered |
|---|---|
| `openCodeLike` | `opencode`, `mimocode`, `deveco`, `openchamber` (+ WSL) |
| `piLike` / `piLikeKeyed` | `pi`, `omo`, `mu`, `prime-agent`, `stepcode` (hybrid) |
| `claudeIn` (env block) | `claude`, `claude#*` profiles, desktop rows — but sign-in/keychain/MCP is bespoke |
| `codexIn` / `codexProfileIn` | `codex`, WSL codex |
| `geminiIn` | `gemini` (`.env` + settings) |
| `crushAt` | `crush` |
| `gooseIn` | `goose` (YAML + custom provider file) |
| `qoderAt`, `workbuddyBuild`, `vscodeOf`, `ompAt`, `dshOn` | 2–3 agents each |
| "one JSON/TOML provider + one model field" bespoke | `zed`, `cursor`, `copilot`, `fx`, `devin`, `empryo`, `nanobot`, `paimon`, `ante`, `astrbot`, `openclaw`, `jcode`, `vibe`, `minimax-code`, `qoder`, `grok`, `raycast`, `openbot`, `craft-agents`, `atomcode`, `ocr`, `snow`, … |

### Not portable as pure config editing

`aside` (runtime settings API), `newmax` (SQLite), `kun` / `avibe` /
`hanako` / `shelley` / `alma` (app admin APIs or IPC sockets), `zed` and
`hanako` (credential store/keychain), `agy` and `dim` and `cursor-local`
(shell functions / launch env), `aionui` / `cherrystudio` / `cindy` /
`lody` (deeplinks only), `claudeDesktop` (encrypted config library + MCP),
`dsh` (its own key store and patch rows), `t3code` (harness `-c` args),
`kirocrew` (connects another agent as an ACP harness), plus every
plugin-defined agent (arbitrary JS via Magpie's plugin runtime).

### Suggested staging

1. **Now**: 4 agents (done) — proves the core, merge policy and stash.
2. **Next**: port the shared shapes (`openCodeLike` family, `piLike` family,
   `codexIn`, `crushAt`, `gooseIn`, `geminiIn`) — roughly 12–15 agents for
   the cost of ~6 shapes.
3. **Then**: the "one provider block + one model field" bespoke JSON/TOML
   agents — each is a registry line plus a small shape, ~45 agents.
4. **Optional connectors**: HTTP/socket/SQLite adapters for the runtime
   agents, only where you control the running app.
5. **Out of scope**: magpie's gateway features (routing groups, failover,
   subscriptions), its JS plugin runtime, and keychain-backed subscriptions.

## Limitations of the current version

- Pi-family aliases use the shape, but per-agent quirks (Pi's extension and
  protocol fields, mu/prime's gateway keys) are not ported yet.
- OpenCode v2 migration is detected from `providers` in the file or
  `opencode_version = "2"` in that agent's table, not by probing the binary.
- The Codex catalog carries Codex's own bundled instruction prompt
  (`injector/assets/codex_prompt.md`, from yetone/magpie, originally
  openai/codex Apache-2.0) because Codex rejects entries without it.
- No YAML round-trip editor yet — needed for Goose, Hermes, Raycast, dsh,
  etc. (`ruamel.yaml` is the intended dependency).

## Attribution

Config shapes and preservation rules were learned from
`github.com/yetone/magpie` (MIT). No magpie code is vendored except the
Codex instruction text noted above.
