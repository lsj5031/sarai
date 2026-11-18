"""Agent-agnostic core: merge policy, stash, and change reporting."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
STATE = ROOT / ".state"


@dataclass
class Change:
    agent: str
    path: str
    action: str
    detail: str = ""


@dataclass
class Report:
    changes: list[Change] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)

    def add(self, agent: str, path: str, action: str, detail: str = "") -> None:
        self.changes.append(Change(agent, str(path), action, detail))

    def error(self, msg: str) -> None:
        self.errors.append(msg)

    def __bool__(self) -> bool:
        return bool(self.changes or self.errors)


def plain(value: Any) -> Any:
    """Convert tomlkit/json containers into plain JSON-safe values."""
    unwrap = getattr(value, "unwrap", None)
    if callable(unwrap) and not isinstance(value, (str, int, float, bool)):
        try:
            value = unwrap()
        except (AttributeError, TypeError, ValueError):
            pass
    if isinstance(value, dict):
        return {str(k): plain(v) for k, v in value.items()}
    if isinstance(value, (list, tuple)):
        return [plain(v) for v in value]
    if isinstance(value, (str, int, float, bool)) or value is None:
        return value
    return str(value)


def merge_preserve(ours: Any, theirs: Any, whole: tuple[str, ...] = ()) -> Any:
    """Merge our block over what is in the file.

    Keys we do not write stay the user's; keys named in `whole` (e.g. the
    model list) are ours alone; everything else we own at the leaf.
    """
    if not isinstance(ours, dict) or not isinstance(theirs, dict):
        return plain(ours)
    out = {k: plain(v) for k, v in ours.items()}
    for key, value in theirs.items():
        if key not in out:
            out[key] = plain(value)
        elif key in whole:
            continue
        elif isinstance(out[key], dict) and isinstance(value, dict):
            out[key] = merge_preserve(out[key], value)
    return out


def expand(path: str) -> Path:
    return Path(os.path.expanduser(os.path.expandvars(path)))


def first_existing(paths: list[Path]) -> Path | None:
    for p in paths:
        if p.exists():
            return p
    return None


class Stash:
    """Remembers the values we replaced, so --restore can put them back."""

    def __init__(self, *, enabled: bool = True):
        self.path = STATE / "stash.json"
        self.enabled = enabled
        self.data: dict[str, dict[str, Any]] = {}
        if self.path.exists():
            try:
                self.data = json.loads(self.path.read_text(encoding="utf-8"))
            except (json.JSONDecodeError, OSError):
                self.data = {}

    def get(self, agent: str, key: str, default: Any = None) -> Any:
        return self.data.get(agent, {}).get(key, default)

    def has(self, agent: str, key: str) -> bool:
        return key in self.data.get(agent, {})

    def remember(self, agent: str, key: str, value: Any) -> None:
        if not self.enabled or self.has(agent, key):
            return
        self.data.setdefault(agent, {})[key] = plain(value)
        self.save()

    def forget(self, agent: str, *keys: str) -> None:
        entry = self.data.get(agent)
        if not entry:
            return
        for key in keys:
            entry.pop(key, None)
        if not entry:
            self.data.pop(agent, None)
        self.save()

    def save(self) -> None:
        if not self.enabled:
            return
        STATE.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(".tmp")
        tmp.write_text(json.dumps(self.data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        os.replace(tmp, self.path)
