"""Codex shape: [model_providers.<id>] in config.toml + a model catalog.

The catalog is what Codex needs for /model; without `model_provider` and
its table together Codex refuses to load config.toml at all, so both are
always written as a pair.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

from .. import assets
from ..core import expand, plain
from ..definition import Definition, Model
from ..tomlfile import TomlFile
from .base import Context, restore_key, restore_ready

CATALOG = "magpie-models.json"
LEVELS = ("minimal", "low", "medium", "high", "xhigh", "max")


def directory() -> Path:
    home = os.getenv("CODEX_HOME")
    return expand(home) if home else Path.home() / ".codex"


def provider_table(defn: Definition) -> dict:
    return {
        "name": defn.provider,
        "base_url": defn.v1,
        "wire_api": "responses",
        "experimental_bearer_token": defn.api_key,
        # Codex only offers its image tool to a provider it reads as the
        # OpenAI actor. Without this header a Codex on a custom provider
        # never gets the image tool.
        "http_headers": {"x-openai-actor-authorization": defn.provider},
    }


def supported_efforts(m: Model) -> list[str]:
    return [e for e in LEVELS if e in m.efforts]


def default_effort(m: Model) -> str:
    efforts = supported_efforts(m)
    return m.default_effort if m.default_effort in efforts else (efforts[0] if efforts else "")


def catalog_entry(defn: Definition, m: Model, priority: int) -> dict:
    efforts = supported_efforts(m)
    return {
        "slug": m.id,
        "display_name": m.name or m.id,
        "description": (m.name or m.id) + " via " + defn.provider,
        "base_instructions": assets.CODEX_PROMPT,
        "default_reasoning_level": default_effort(m) or None,
        "supported_reasoning_levels": [{"effort": e, "description": e} for e in efforts],
        "shell_type": "unified_exec",
        "visibility": "list",
        "supported_in_api": True,
        "priority": priority,
        "support_verbosity": False,
        "apply_patch_tool_type": "freeform",
        "truncation_policy": {"mode": "tokens", "limit": 10000},
        "experimental_supported_tools": [],
        "input_modalities": ["text", "image"] if m.images else ["text"],
        "context_window": m.context or None,
        "effective_context_window_percent": 95,
        "service_tiers": [],
        "supports_search_tool": True,
        "supports_parallel_tool_calls": True,
        "supports_reasoning_summaries": bool(efforts),
    }


def write_catalog(defn: Definition, path: Path) -> bool:
    ours = [catalog_entry(defn, m, i + 1) for i, m in enumerate(defn.models)]
    keep: list[dict] = []
    if path.exists():
        try:
            old = json.loads(path.read_text(encoding="utf-8-sig"))
            # Codex's model_catalog_json is an object: {"models": [...]}.
            entries = old.get("models", []) if isinstance(old, dict) else old
            if isinstance(entries, list):
                slugs = {e["slug"] for e in ours}
                keep = [e for e in entries if isinstance(e, dict) and e.get("slug") not in slugs]
        except (json.JSONDecodeError, OSError):
            pass
    data = {"models": plain(keep + ours)}
    text = json.dumps(data, indent=2, ensure_ascii=False) + "\n"
    if path.exists() and path.read_text(encoding="utf-8-sig") == text:
        return False
    tmp = path.with_suffix(".json.tmp")
    tmp.write_text(text, encoding="utf-8")
    os.replace(tmp, path)
    return True


def apply(ctx: Context) -> None:
    m = ctx.model
    if m is None:
        return
    effort = default_effort(m)
    d = directory()
    config = d / "config.toml"
    catalog = d / CATALOG
    table_path = ("model_providers", ctx.provider)
    detail = (f"[model_providers.{ctx.provider}], model_provider, model, model_catalog_json; "
              + (f"model_reasoning_effort = {effort}" if effort else "clear model_reasoning_effort"))
    if ctx.dry_run:
        ctx.note(str(config), "would set", detail)
        ctx.note(str(catalog), "would write", f"{len(ctx.defn.models)} entries")
        return
    d.mkdir(parents=True, exist_ok=True)
    ctx.mark_applied()
    f = TomlFile(config, create=True)
    existing = f.get(*table_path)
    ctx.stash.remember(ctx.agent, "provider_table", existing)
    for key in ("model_provider", "model", "model_catalog_json", "model_reasoning_effort"):
        ctx.stash.remember(ctx.agent, key, f.get(key))
    f.set(provider_table(ctx.defn), *table_path)
    f.set(ctx.provider, "model_provider")
    f.set(m.id, "model")
    f.set(CATALOG, "model_catalog_json")
    if effort:
        f.set(effort, "model_reasoning_effort")
    else:
        f.delete("model_reasoning_effort")
    if f.save():
        ctx.note(str(config), "set", detail)
    if not catalog.exists():
        ctx.stash.remember(ctx.agent, "catalog_absent", True)
    if write_catalog(ctx.defn, catalog):
        ctx.note(str(catalog), "wrote", f"{len(ctx.defn.models)} entries")


def restore(ctx: Context) -> None:
    d = directory()
    if not restore_ready(ctx, str(d)):
        return
    config = d / "config.toml"
    if config.exists():
        f = TomlFile(config)
        restore_key(f, ctx, "provider_table", "model_providers", ctx.provider)
        for key in ("model_provider", "model", "model_catalog_json", "model_reasoning_effort"):
            restore_key(f, ctx, key, key)
        if f.save():
            ctx.note(str(config), "restored")
    catalog = d / CATALOG
    if catalog.exists():
        if ctx.stash.get(ctx.agent, "catalog_absent"):
            catalog.unlink()
            ctx.note(str(catalog), "removed")
        else:
            # take our entries out, leave anything the user added
            try:
                old = json.loads(catalog.read_text(encoding="utf-8-sig"))
                entries = old.get("models", []) if isinstance(old, dict) else old
                ours = {m.id for m in ctx.defn.models}
                kept = [e for e in entries if isinstance(e, dict) and e.get("slug") not in ours]
                catalog.write_text(
                    json.dumps({"models": kept}, indent=2, ensure_ascii=False) + "\n",
                    encoding="utf-8",
                )
                ctx.note(str(catalog), "restored")
            except (json.JSONDecodeError, OSError):
                pass
    ctx.stash.forget(ctx.agent, "applied", "provider_table", "model_provider", "model",
                     "model_catalog_json", "model_reasoning_effort", "catalog_absent")
