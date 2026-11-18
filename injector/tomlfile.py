"""TOML editor that keeps comments, ordering and whitespace intact."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

import tomlkit


BOM = b"\xef\xbb\xbf"


class TomlFile:
    def __init__(self, path: str | Path, *, create: bool = False):
        self.path = Path(path)
        self.bom = False
        self.newline = "\n"
        if self.path.exists():
            raw = self.path.read_bytes()
            self.bom = raw.startswith(BOM)
            text = raw.decode("utf-8-sig")
            if "\r\n" in text:
                # tomlkit works in LF; the file's own line endings are
                # re-applied on save so an apply does not rewrite them.
                self.newline = "\r\n"
                text = text.replace("\r\n", "\n")
            self.doc = tomlkit.parse(text)
        else:
            if not create:
                raise FileNotFoundError(str(self.path))
            self.doc = tomlkit.document()

    def _walk(self, *path: str, create: bool) -> Any:
        node: Any = self.doc
        for i, part in enumerate(path):
            last = i == len(path) - 1
            nxt = node.get(part)
            if nxt is None:
                if not create:
                    return None
                nxt = tomlkit.table()
                node[part] = nxt
            # Only intermediate parts must be tables; the final value may be
            # a scalar (that is what get() is for).
            if not last and not isinstance(nxt, dict):
                return None
            node = nxt
        return node

    def has(self, *path: str) -> bool:
        return self._walk(*path, create=False) is not None

    def get(self, *path: str) -> Any:
        if not path:
            return self.doc
        return self._walk(*path, create=False)

    def table(self, *path: str) -> dict[str, Any]:
        """Get or create a table at path."""
        return self._walk(*path, create=True)

    def set(self, value: Any, *path: str) -> None:
        if not path:
            raise ValueError("empty path")
        parent = self._walk(*path[:-1], create=True)
        parent[path[-1]] = value

    def delete(self, *path: str) -> bool:
        if not path:
            return False
        parent = self._walk(*path[:-1], create=False)
        if parent is None or path[-1] not in parent:
            return False
        del parent[path[-1]]
        return True

    def save(self) -> bool:
        text = tomlkit.dumps(self.doc)
        if self.newline != "\n":
            text = text.replace("\r\n", "\n").replace("\n", self.newline)
        data = (BOM if self.bom else b"") + text.encode("utf-8")
        if self.path.exists() and self.path.read_bytes() == data:
            return False
        tmp = self.path.with_name(self.path.name + ".tmp")
        tmp.write_bytes(data)
        os.replace(tmp, self.path)
        return True
