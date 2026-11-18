"""Agent id -> shape driver.

Adding an agent is a registry line, not a new editor. Once more shapes are
ported, this is where the rest of magpie's agent list plugs in.
"""

from __future__ import annotations

from typing import Callable

from .shapes import codex, droid, opencode, pi
from .shapes.base import Context

Driver = Callable[[Context], None]
Entry = tuple[Driver, Driver]  # (apply, restore)


PI_DIRS = {
    "pi": "~/.pi/agent",
    "omo": "~/.omo/agent",
    "mu": "~/.mu/agent",
    "stepcode": "~/.stepcode",
    "prime-agent": "~/.prime/agent",
}


def _pi_dir(ctx: Context) -> str:
    return str(ctx.choice.options.get("directory", PI_DIRS.get(ctx.agent, "~/.pi/agent")))


def _pi_apply(ctx: Context) -> None:
    pi.apply(ctx, _pi_dir(ctx))


def _pi_restore(ctx: Context) -> None:
    pi.restore(ctx, _pi_dir(ctx))


def _oc_apply(ctx: Context) -> None:
    opencode.apply(ctx, agent_dir=ctx.choice.options.get("directory"))


def _oc_restore(ctx: Context) -> None:
    opencode.restore(ctx, agent_dir=ctx.choice.options.get("directory"))


REGISTRY: dict[str, Entry] = {
    "pi": (_pi_apply, _pi_restore),
    "droid": (droid.apply, droid.restore),
    "codex": (codex.apply, codex.restore),
    "opencode": (_oc_apply, _oc_restore),
}

# Aliases for the Pi-family forks that share the same files and fields.
for _alias in ("omo", "mu", "stepcode", "prime-agent"):
    REGISTRY[_alias] = REGISTRY["pi"]
for _alias in ("mimocode", "deveco"):
    REGISTRY[_alias] = REGISTRY["opencode"]


def supported() -> list[str]:
    return sorted(REGISTRY)
