"""Pi shape: settings.json (selection) + models.json (provider + catalog).

Shared by Pi, OmO, mu, StepCode and other Pi-family agents; the paths and a
few field names are passed in by the registry.
"""

from __future__ import annotations

import fnmatch
import os
from pathlib import Path

from ..core import expand, merge_preserve
from ..definition import Definition, Model
from ..textjson import JsonFile
from .base import Context

LEAVES = ("defaultProvider", "defaultModel", "defaultThinkingLevel", "enabledModels")


def paths(directory: str) -> tuple[Path, Path]:
    d = expand(directory)
    return d / "settings.json", d / "models.json"


def model_entry(defn: Definition, m: Model) -> dict:
    e: dict = {"id": m.id, "name": m.name or m.id, "reasoning": m.reasoning or bool(m.efforts)}
    if m.speaks_anthropic:
        e["api"] = "anthropic-messages"
        e["baseUrl"] = defn.root
    elif m.speaks_responses:
        e["api"] = "openai-responses"
    if m.context:
        e["contextWindow"] = m.context
    if m.output:
        e["maxTokens"] = m.output
    if m.images or m.input_modalities:
        e["input"] = ["text", "image"] if m.images else m.input_modalities
    if m.efforts:
        e["thinkingLevelMap"] = {level: level for level in m.efforts}
    return e


def provider_block(defn: Definition, models: list[Model]) -> dict:
    return {
        "name": defn.provider,
        "baseUrl": defn.v1,
        "api": "openai-completions",
        "apiKey": defn.api_key,
        "compat": {"sendSessionAffinityHeaders": True},
        "models": [model_entry(defn, m) for m in models],
    }


def _wanted(ref: str, pats: list) -> bool:
    """Does the user's enabledModels already take in ref?"""
    if not pats:
        return False
    pats = [str(p) for p in pats]
    candidate = ref.lower()
    short = candidate.split("/", 1)[-1]
    for pat in pats:
        p = pat.lower().strip()
        if p == candidate or p == short:
            return True
        if any(ch in p for ch in "*?["):
            if fnmatch.fnmatch(candidate, p) or fnmatch.fnmatch(short, p):
                return True
    return False


def apply(ctx: Context, directory: str, *, all_models: bool = True) -> None:
    m = ctx.model
    if m is None:
        return
    settings, models_path = paths(directory)
    models_path.parent.mkdir(parents=True, exist_ok=True)

    mf = JsonFile(models_path, create=True)
    key = ("providers", ctx.provider)
    existing = mf.get(*key)
    ours = provider_block(ctx.defn, ctx.defn.models if all_models else [m])
    # Keep the block byte-stable across repeated runs: every key we own is
    # always written, in the same order, and only unknown keys are carried
    # over by merge_preserve.
    merged = merge_preserve(ours, existing, whole=("models",))
    if ctx.dry_run:
        ctx.note(str(models_path), "would set", ".".join(key))
    else:
        ctx.stash.remember(ctx.agent, "models_block", existing)
        mf.set(merged, *key)
        if mf.save():
            ctx.note(str(models_path), "set", ".".join(key))

    sf = JsonFile(settings, create=True)
    if ctx.dry_run:
        ctx.note(str(settings), "would set", "defaultProvider/defaultModel")
        return

    for leaf in LEAVES:
        if not ctx.stash.has(ctx.agent, leaf):
            ctx.stash.remember(ctx.agent, leaf, sf.get(leaf))
    sf.set(ctx.provider, "defaultProvider")
    sf.set(m.id, "defaultModel")

    scope = sf.get("enabledModels")
    if isinstance(scope, list):
        scope = [str(x) for x in scope]
        if not _wanted(ctx.ref, scope):
            sf.set(scope + [ctx.ref], "enabledModels")

    effort = str(ctx.choice.options.get("effort", ""))
    if effort:
        sf.set(effort, "defaultThinkingLevel")

    if sf.save():
        ctx.note(str(settings), "set", "defaultProvider/defaultModel")


def restore(ctx: Context, directory: str) -> None:
    settings, models_path = paths(directory)
    if models_path.exists():
        mf = JsonFile(models_path)
        had = ctx.stash.get(ctx.agent, "models_block")
        if had is None:
            mf.delete("providers", ctx.provider)
        else:
            mf.set(had, "providers", ctx.provider)
        if mf.save():
            ctx.note(str(models_path), "restored")

    if settings.exists():
        sf = JsonFile(settings)
        for leaf in LEAVES:
            if ctx.stash.has(ctx.agent, leaf):
                old = ctx.stash.get(ctx.agent, leaf)
                if old is None:
                    sf.delete(leaf)
                else:
                    sf.set(old, leaf)
        if sf.save():
            ctx.note(str(settings), "restored")
    ctx.stash.forget(ctx.agent, *LEAVES, "models_block")
