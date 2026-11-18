from __future__ import annotations

import argparse
import sys

from .core import Report, Stash
from .definition import AgentChoice, load
from .registry import REGISTRY, supported
from .shapes.base import Context


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="agent-config-injector",
        description="Inject your own provider and models into coding agents' config files.",
    )
    ap.add_argument("-d", "--def", dest="definition", default="defs/example.toml",
                    help="definition file (.toml/.yaml/.json)")
    ap.add_argument("-a", "--agents", help="comma-separated agent ids (default: all in the definition)")
    ap.add_argument("--restore", action="store_true", help="put back what was there before")
    ap.add_argument("--dry-run", action="store_true", help="show what would change without writing")
    ap.add_argument("--no-stash", action="store_true", help="do not remember original values")
    ap.add_argument("--list", action="store_true", help="list supported agents and exit")
    args = ap.parse_args(argv)

    if args.list:
        print("supported agents:")
        for name in supported():
            print("  " + name)
        return 0

    defn = load(args.definition)
    agents = (
        [a.strip() for a in args.agents.split(",") if a.strip()]
        if args.agents
        else list(defn.agents)
    )
    stash = Stash(enabled=not args.no_stash and not args.dry_run)
    report = Report()

    for agent in agents:
        if agent not in REGISTRY:
            report.error(f"{agent}: not supported (see --list)")
            continue
        choice = defn.agents.get(agent, AgentChoice())
        ctx = Context(defn, agent, choice, stash, report, args.dry_run)
        apply, restore = REGISTRY[agent]
        try:
            (restore if args.restore else apply)(ctx)
        except Exception as exc:  # one bad agent must not stop the rest
            report.error(f"{agent}: {type(exc).__name__}: {exc}")

    if not report.changes:
        print("nothing to do")
    for c in report.changes:
        tail = f"  ({c.detail})" if c.detail else ""
        print(f"{c.agent}: {c.action} {c.path}{tail}")
    for err in report.errors:
        print("error: " + err, file=sys.stderr)
    return 1 if report.errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
