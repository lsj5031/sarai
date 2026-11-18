from __future__ import annotations

from dataclasses import dataclass

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
