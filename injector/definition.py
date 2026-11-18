"""Load the user's own provider/model definition (TOML first, YAML/JSON too).

Nothing in here depends on magpie: `provider.id`, `gateway` and `models` come
entirely from the definition file the caller supplies.
"""

from __future__ import annotations

import json
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class Model:
    id: str
    name: str = ""
    context: int = 0
    output: int = 0
    reasoning: bool = False
    efforts: list[str] = field(default_factory=list)
    default_effort: str = ""
    apis: list[str] = field(default_factory=list)   # chat | responses | anthropic
    images: bool = False
    input_modalities: list[str] = field(default_factory=list)

    @property
    def speaks_anthropic(self) -> bool:
        return "anthropic" in self.apis

    @property
    def speaks_responses(self) -> bool:
        return "responses" in self.apis


@dataclass
class AgentChoice:
    model: str = ""
    options: dict[str, Any] = field(default_factory=dict)


@dataclass
class Definition:
    provider: str
    api_key: str
    v1: str                     # Chat Completions / Responses base URL
    root: str                   # gateway root, for Anthropic Messages
    models: list[Model]
    agents: dict[str, AgentChoice]

    def model(self, model_id: str) -> Model:
        for m in self.models:
            if m.id == model_id:
                return m
        raise KeyError(f"model {model_id!r} is not in the definition")

    def chosen(self, agent: str) -> Model | None:
        choice = self.agents.get(agent)
        if choice is None or not choice.model:
            return None
        try:
            return self.model(choice.model)
        except KeyError:
            return None


def _read(path: Path) -> dict[str, Any]:
    suffix = path.suffix.lower()
    if suffix == ".toml":
        with path.open("rb") as fh:
            return tomllib.load(fh)
    if suffix == ".json":
        return json.loads(path.read_text(encoding="utf-8-sig"))
    if suffix in (".yaml", ".yml"):
        try:
            import yaml  # type: ignore
        except ImportError as exc:  # pragma: no cover
            raise SystemExit("YAML definitions need PyYAML: pip install pyyaml") from exc
        return yaml.safe_load(path.read_text(encoding="utf-8-sig")) or {}
    raise SystemExit(f"unsupported definition format: {suffix} (use .toml, .yaml or .json)")


def _as_choice(value: Any) -> AgentChoice:
    if isinstance(value, str):
        return AgentChoice(model=value)
    if isinstance(value, dict):
        return AgentChoice(model=str(value.get("model", "")), options=dict(value))
    raise SystemExit(f"agent entry must be a model id or a table, got {value!r}")


def load(path: str | Path) -> Definition:
    raw = _read(Path(path))

    provider = str(raw.get("provider", {}).get("id", "magpie"))
    gw = raw.get("gateway") or {}
    base = str(gw.get("base_url", "")).rstrip("/")
    if not base:
        raise SystemExit("definition needs gateway.base_url")
    v1 = base if base.endswith("/v1") else base + "/v1"
    root = base[: -len("/v1")] if base.endswith("/v1") else base
    api_key = str(gw.get("api_key", ""))

    models: list[Model] = []
    for item in raw.get("models", []) or []:
        if not isinstance(item, dict) or not item.get("id"):
            raise SystemExit(f"bad [[models]] entry: {item!r}")
        models.append(
            Model(
                id=str(item["id"]),
                name=str(item.get("name", "")),
                context=int(item.get("context", 0) or 0),
                output=int(item.get("output", 0) or 0),
                reasoning=bool(item.get("reasoning", False)),
                efforts=[str(x) for x in (item.get("efforts") or [])],
                default_effort=str(item.get("default_effort", "")),
                apis=[str(x) for x in (item.get("apis") or [])],
                images=bool(item.get("images", False)),
                input_modalities=[str(x) for x in (item.get("input_modalities") or [])],
            )
        )
    if not models:
        raise SystemExit("definition has no [[models]] entries")

    known = {m.id for m in models}
    agents: dict[str, AgentChoice] = {}
    for name, value in (raw.get("agents") or {}).items():
        choice = _as_choice(value)
        if not choice.model:
            raise SystemExit(f"[agents].{name} needs a model id")
        if choice.model not in known:
            raise SystemExit(f"[agents].{name}: model {choice.model!r} is not in [[models]]")
        agents[str(name)] = choice
    return Definition(provider, api_key, v1, root, models, agents)
