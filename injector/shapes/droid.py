"""Droid / Factory shape: BYOK customModels in ~/.factory/settings.json."""

from __future__ import annotations

import os
from pathlib import Path

from ..core import expand
from ..definition import Definition, Model
from ..textjson import JsonFile
from .base import Context, restore_key, restore_ready

MODEL_KEY = "sessionDefaultSettings.model"


def path() -> Path:
    override = os.getenv("FACTORY_HOME_OVERRIDE")
    home = expand(override) if override else Path.home()
    return home / ".factory" / "settings.json"


def owned_id(defn: Definition, m: Model) -> str:
    return f"custom:{defn.provider}/{m.id}"


def entry(defn: Definition, m: Model) -> dict:
    if m.speaks_anthropic:
        provider = "anthropic"
    elif m.speaks_responses:
        provider = "openai"
    else:
        provider = "generic-chat-completion-api"
    e: dict = {
        "model": m.id,
        "id": owned_id(defn, m),
        "displayName": m.name or m.id,
        "baseUrl": defn.root if provider == "anthropic" else defn.v1,
        "apiKey": defn.api_key,
        "provider": provider,
    }
    if m.context:
        e["maxContextLimit"] = m.context
    if m.output:
        e["maxOutputTokens"] = m.output
    if m.efforts:
        order = ["medium", "high", "low", "xhigh", "max", "minimal"]
        default = m.default_effort
        e["reasoningEffort"] = default if default in m.efforts and default in order else next(
            (x for x in order if x in m.efforts), "medium"
        )
    e["noImageSupport"] = not m.images
    return e


def apply(ctx: Context) -> None:
    m = ctx.model
    if m is None:
        return
    p = path()
    our = [entry(ctx.defn, model) for model in ctx.defn.models]
    if ctx.dry_run:
        ctx.note(str(p), "would set", f"customModels ({len(our)}), {MODEL_KEY}")
        return
    p.parent.mkdir(parents=True, exist_ok=True)
    ctx.mark_applied()
    f = JsonFile(p, create=True)
    prefix = f"custom:{ctx.provider}/"
    keep = []
    for item in f.get("customModels") or []:
        if isinstance(item, dict) and str(item.get("id", "")).startswith(prefix):
            continue
        keep.append(item)
    ctx.stash.remember(ctx.agent, "customModels", f.get("customModels"))
    ctx.stash.remember(ctx.agent, "model", f.get(*MODEL_KEY.split(".")))
    f.set(keep + our, "customModels")
    f.set(owned_id(ctx.defn, m), *MODEL_KEY.split("."))
    if f.save():
        ctx.note(str(p), "set", f"customModels ({len(our)}), {MODEL_KEY}")


def restore(ctx: Context) -> None:
    p = path()
    if not restore_ready(ctx, str(p)):
        return
    if not p.exists():
        ctx.stash.forget(ctx.agent, "applied", "customModels", "model")
        return
    f = JsonFile(p)
    restore_key(f, ctx, "customModels", "customModels")
    restore_key(f, ctx, "model", *MODEL_KEY.split("."))
    if f.save():
        ctx.note(str(p), "restored")
    ctx.stash.forget(ctx.agent, "applied", "customModels", "model")
