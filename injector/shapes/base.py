from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Protocol

from ..core import Report, Stash
from ..definition import AgentChoice, Definition, Model


@dataclass
class Context:
    defn: Definition
    agent: str
    choice: AgentChoice
    stash: Stash
    report: Report
    dry_run: bool = False

    @property
    def provider(self) -> str:
        return self.defn.provider

    @property
    def model(self) -> Model | None:
        return self.defn.chosen(self.agent)

    @property
    def ref(self) -> str:
        m = self.model
        return f"{self.provider}/{m.id}" if m else ""

    def note(self, path: str, action: str, detail: str = "") -> None:
        self.report.add(self.agent, path, action, detail)

    def mark_applied(self) -> None:
        """Record that this agent has something stashed to undo later."""
        self.stash.remember(self.agent, "applied", True)


class Editor(Protocol):
    def set(self, value: Any, *path: str) -> Any: ...
    def delete(self, *path: str) -> bool: ...


def restore_ready(ctx: Context, path: str) -> bool:
    """Only undo what this checkout applied, and never write on a dry run."""
    if not ctx.stash.has(ctx.agent, "applied"):
        ctx.note(path, "restore skipped", "no stashed originals in .state")
        return False
    if ctx.dry_run:
        ctx.note(path, "would restore")
        return False
    return True


def restore_key(editor: Editor, ctx: Context, key: str, *path: str) -> None:
    """Put one stashed key back, or delete it when it was absent before."""
    if not ctx.stash.has(ctx.agent, key):
        return
    old = ctx.stash.get(ctx.agent, key)
    if old is None:
        editor.delete(*path)
    else:
        editor.set(old, *path)
