"""OpenCode shape: a `magpie` provider plus model/small_model fields.

Handles both OpenCode 1 (`provider.<id>` with `options`) and OpenCode 2
(`providers.<id>` with `settings`) without needing the CLI installed: the
presence of a `providers` key, or an explicit `opencode_version=2` option,
selects v2. Shared by OpenCode, MiMo Code, DevEco Code and forks.
"""

from __future__ import annotations

import os
from pathlib import Path

from ..core import expand, merge_preserve
from ..definition import Definition, Model
from ..textjson import JsonFile
from .base import Context

PACKAGE = "aisdk:@ai-sdk/openai-compatible"


def directory(agent: str, config_home: str | None = None) -> Path:
    override = os.getenv("OPENCODE_CONFIG_DIR")
    if override and agent == "opencode":
        return expand(override)
    if config_home:
        return expand(config_home) / agent
    xdg = os.getenv("XDG_CONFIG_HOME")
    return expand(xdg) / agent if xdg else Path.home() / ".config" / agent


def config_path(directory: Path, agent: str) -> Path:
    for name in (f"{agent}.jsonc", f"{agent}.json", "config.json"):
        p = directory / name
        if p.exists():
            return p
    return directory / f"{agent}.json"


def variants(m: Model) -> dict:
    order = ["minimal", "low", "medium", "high", "xhigh", "max"]
    efforts = [e for e in order if e in m.efforts] or list(m.efforts)
    return {e: {"reasoningEffort": e} for e in efforts}


def v1_model(defn: Definition, m: Model) -> dict:
    e: dict = {"name": m.name or m.id}
    if m.images:
        e["modalities"] = {"input": ["text", "image"], "output": ["text"]}
    if m.context or m.output:
        e["limit"] = {"context": m.context, "output": m.output}
    if m.reasoning or m.efforts:
        e["reasoning"] = True
    if v := variants(m):
        e["variants"] = v
    return e


def v2_model(defn: Definition, m: Model) -> dict:
    e: dict = {"name": m.name or m.id}
    if m.images:
        e["capabilities"] = {"tools": True, "input": ["text", "image"], "output": ["text"]}
    if m.context or m.output:
        e["limit"] = {"context": m.context, "output": m.output}
    if v := variants(m):
        e["variants"] = [
            {"id": name, "settings": opts, "body": {}}
            if name.endswith("-fast")
            else {"id": name, "settings": opts}
            for name, opts in v.items()
        ]
    return e


def block(defn: Definition, two: bool) -> dict:
    models = [v2_model(defn, m) if two else v1_model(defn, m) for m in defn.models]
    if two:
        return {"package": PACKAGE, "name": defn.provider,
                "settings": {"baseURL": defn.v1, "apiKey": defn.api_key},
                "models": {m.id: e for m, e in zip(defn.models, models)}}
    return {"npm": "@ai-sdk/openai-compatible", "name": defn.provider,
            "options": {"baseURL": defn.v1, "apiKey": defn.api_key, "setCacheKey": True},
            "models": {m.id: e for m, e in zip(defn.models, models)}}


def is_v2(f: JsonFile, choice_options: dict) -> bool:
    if str(choice_options.get("opencode_version", "")) in ("2", "v2"):
        return True
    return f.has("providers")


def apply(ctx: Context, *, agent_dir: str | None = None) -> None:
    m = ctx.model
    if m is None:
        return
    p = config_path(directory(ctx.agent, agent_dir), ctx.agent)
    p.parent.mkdir(parents=True, exist_ok=True)
    f = JsonFile(p, create=True)
    two = is_v2(f, ctx.choice.options)
    key = ("providers" if two else "provider", ctx.provider)
    existing = f.get(*key)
    ours = block(ctx.defn, two)
    merged = merge_preserve(ours, existing, whole=("models",))
    if ctx.dry_run:
        ctx.note(str(p), "would set", ".".join(key))
        return
    ctx.stash.remember(ctx.agent, "provider_block", existing)
    ctx.stash.remember(ctx.agent, "model", f.get("model"))
    ctx.stash.remember(ctx.agent, "small_model", f.get("small_model"))
    f.set(merged, *key)
    f.set(f"{ctx.provider}/{m.id}", "model")
    if str(ctx.choice.options.get("small", "same")).lower() != "none":
        f.set(f"{ctx.provider}/{m.id}", "small_model")
    if f.save():
        ctx.note(str(p), "set", f"{'.'.join(key)}, model, small_model")


def restore(ctx: Context, *, agent_dir: str | None = None) -> None:
    p = config_path(directory(ctx.agent, agent_dir), ctx.agent)
    if not p.exists():
        return
    f = JsonFile(p)
    had = ctx.stash.get(ctx.agent, "provider_block")
    for section in ("provider", "providers"):
        if had is None:
            f.delete(section, ctx.provider)
        elif f.has(section, ctx.provider):
            f.set(had, section, ctx.provider)
    for key in ("model", "small_model"):
        old = ctx.stash.get(ctx.agent, key)
        if old is None:
            f.delete(key)
        else:
            f.set(old, key)
    if f.save():
        ctx.note(str(p), "restored")
    ctx.stash.forget(ctx.agent, "provider_block", "model", "small_model")
