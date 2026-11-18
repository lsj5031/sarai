"""A JSON/JSONC editor that only touches the bytes it has to.

Magpie's `internal/edit` does surgical edits so comments, key order,
indentation and CRLF in an agent's config survive. This is the Python
equivalent: parse once to find exact value spans, then splice text.

Paths are given as separate components, e.g. f.set("provider", "mygw", ...).
"""

from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class Node:
    start: int
    end: int


@dataclass
class Scalar(Node):
    value: Any


@dataclass
class Member:
    key: str
    key_start: int
    key_end: int
    value: Node


@dataclass
class Obj(Node):
    members: list[Member] = field(default_factory=list)


@dataclass
class Arr(Node):
    items: list[Node] = field(default_factory=list)


class ParseError(ValueError):
    pass


class _Parser:
    def __init__(self, text: str):
        self.s = text
        self.i = 0

    def trivia(self) -> None:
        s = self.s
        while self.i < len(s):
            c = s[self.i]
            if c.isspace():
                self.i += 1
            elif s.startswith("//", self.i):
                end = s.find("\n", self.i)
                self.i = len(s) if end < 0 else end
            elif s.startswith("/*", self.i):
                end = s.find("*/", self.i + 2)
                self.i = len(s) if end < 0 else end + 2
            elif c == "#":
                end = s.find("\n", self.i)
                self.i = len(s) if end < 0 else end
            else:
                break

    def string(self) -> tuple[str, int, int]:
        start = self.i
        s = self.s
        if self.i >= len(s) or s[self.i] != '"':
            raise ParseError(f"expected string at {self.i}")
        self.i += 1
        while self.i < len(s):
            c = s[self.i]
            if c == "\\":
                self.i += 2
                continue
            self.i += 1
            if c == '"':
                try:
                    return json.loads(s[start:self.i]), start, self.i
                except json.JSONDecodeError as exc:
                    raise ParseError(f"bad string at {start}: {exc}") from exc
        raise ParseError(f"unterminated string at {start}")

    def value(self) -> Node:
        self.trivia()
        if self.i >= len(self.s):
            raise ParseError("unexpected end of file")
        c = self.s[self.i]
        if c == "{":
            return self.obj()
        if c == "[":
            return self.arr()
        if c == '"':
            _, start, end = self.string()
            return Scalar(start, end, json.loads(self.s[start:end]))
        start = self.i
        s = self.s
        while self.i < len(s) and s[self.i] not in ",}]":
            if s.startswith("//", self.i) or s.startswith("/*", self.i):
                break
            self.i += 1
        end = self.i
        raw = s[start:end].rstrip()
        end = start + len(raw)
        try:
            v = json.loads(raw)
        except json.JSONDecodeError:
            v = raw
        return Scalar(start, end, v)

    def obj(self) -> Obj:
        start = self.i
        self.i += 1  # '{'
        members: list[Member] = []
        while True:
            self.trivia()
            if self.i >= len(self.s):
                raise ParseError("unterminated object")
            if self.s[self.i] == "}":
                self.i += 1
                return Obj(start, self.i, members)
            key, ks, ke = self.string()
            self.trivia()
            if self.i >= len(self.s) or self.s[self.i] != ":":
                raise ParseError(f"expected ':' at {self.i}")
            self.i += 1
            v = self.value()
            members.append(Member(key, ks, ke, v))
            self.trivia()
            if self.i < len(self.s) and self.s[self.i] == ",":
                self.i += 1
                continue
            if self.i < len(self.s) and self.s[self.i] == "}":
                self.i += 1
                return Obj(start, self.i, members)
            raise ParseError(f"expected ',' or '}}' at {self.i}")

    def arr(self) -> Arr:
        start = self.i
        self.i += 1  # '['
        items: list[Node] = []
        while True:
            self.trivia()
            if self.i >= len(self.s):
                raise ParseError("unterminated array")
            if self.s[self.i] == "]":
                self.i += 1
                return Arr(start, self.i, items)
            items.append(self.value())
            self.trivia()
            if self.i < len(self.s) and self.s[self.i] == ",":
                self.i += 1
                continue
            if self.i < len(self.s) and self.s[self.i] == "]":
                self.i += 1
                return Arr(start, self.i, items)
            raise ParseError(f"expected ',' or ']' at {self.i}")


def _dump(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False)


class JsonFile:
    """Surgical, comment-preserving reader/writer for a JSON(C) file."""

    def __init__(self, path: str | Path, *, create: bool = False):
        self.path = Path(path)
        if self.path.exists():
            raw = self.path.read_bytes()
            self.bom = raw.startswith(b"\xef\xbb\xbf")
            if self.bom:
                raw = raw[3:]
            self.src = raw.decode("utf-8")
        else:
            if not create:
                raise FileNotFoundError(str(self.path))
            self.bom = False
            self.src = "{}"
        self.crlf = "\r\n" in self.src
        self.parse()

    def parse(self) -> None:
        self.root = _Parser(self.src).value()

    # -- reading ---------------------------------------------------------
    def node(self, *path: str) -> Node | None:
        node: Node | None = self.root
        for part in path:
            if isinstance(node, Obj):
                found = next((m for m in node.members if m.key == part), None)
                node = found.value if found else None
            elif isinstance(node, Arr):
                try:
                    idx = int(part)
                except ValueError:
                    return None
                node = node.items[idx] if 0 <= idx < len(node.items) else None
            else:
                return None
            if node is None:
                return None
        return node

    def has(self, *path: str) -> bool:
        return self.node(*path) is not None

    def raw(self, *path: str) -> str | None:
        node = self.node(*path)
        return None if node is None else self.src[node.start:node.end]

    def get(self, *path: str) -> Any:
        node = self.node(*path)
        if node is None:
            return None
        if isinstance(node, Scalar):
            return node.value
        try:
            return json.loads(self.src[node.start:node.end])
        except json.JSONDecodeError:
            return None

    # -- editing ---------------------------------------------------------
    def _indent(self, pos: int) -> str:
        """Leading whitespace of the line containing pos."""
        line = self.src.rfind("\n", 0, pos) + 1
        prefix = self.src[line:pos]
        return re.match(r"[ \t]*", prefix).group(0)  # type: ignore[union-attr]

    def _replace(self, node: Node, text: str) -> None:
        self.src = self.src[: node.start] + text + self.src[node.end :]
        self.parse()

    def _parent(self, *path: str) -> Node:
        node = self.root
        prefix: list[str] = []
        for part in path:
            if isinstance(node, Obj):
                found = next((m for m in node.members if m.key == part), None)
                if found is None:
                    # _insert re-parses, so every old node reference is stale.
                    self._insert(node, part, {})
                    node = self.node(*(prefix + [part]))
                else:
                    node = found.value
            elif isinstance(node, Arr):
                idx = int(part)
                while True:
                    cur = self.node(*prefix)
                    assert isinstance(cur, Arr)
                    if len(cur.items) > idx:
                        break
                    self._append(cur, {})
                node = self.node(*prefix).items[idx]  # type: ignore[union-attr]
            else:
                raise ParseError(f"cannot descend through a scalar at {part!r}")
            prefix.append(part)
        return node

    def _insert(self, obj: Obj, key: str, value: Any) -> None:
        text = _dump(value)
        if obj.members:
            last = obj.members[-1]
            pos = last.value.end
            indent = self._indent(last.key_start) or (self._indent(obj.start) + "  ")
            # A trailing comma after the last member means we append without
            # another comma; otherwise ours must come first.
            sep = "" if "," in self.src[pos : obj.end - 1] else ","
            ins = f"{sep}\n{indent}{_dump(key)}: {text}"
        else:
            pos = obj.start + 1  # just inside '{'
            parent_indent = self._indent(obj.start)
            indent = parent_indent + "  "
            ins = f"\n{indent}{_dump(key)}: {text}\n{parent_indent}"
        self.src = self.src[:pos] + ins + self.src[pos:]
        self.parse()

    def _append(self, arr: Arr, value: Any) -> None:
        pos = arr.end - 1
        indent = self._indent(arr.start) + "  "
        sep = "," if arr.items else ""
        ins = f"{sep}\n{indent}{_dump(value)}"
        self.src = self.src[:pos] + ins + self.src[pos:]
        self.parse()

    def set(self, value: Any, *path: str) -> bool:
        """Set path to value. Returns True when bytes actually changed."""
        if not path:
            raise ValueError("empty path")
        before = self.src
        parent = self._parent(*path[:-1])
        key = path[-1]
        if isinstance(parent, Obj):
            found = next((m for m in parent.members if m.key == key), None)
            if found:
                self._replace(found.value, _dump(value))
            else:
                self._insert(parent, key, value)
        elif isinstance(parent, Arr):
            idx = int(key)
            while True:
                cur = self.node(*path[:-1])
                assert isinstance(cur, Arr)
                if len(cur.items) > idx:
                    break
                self._append(cur, None)
            self._replace(cur.items[idx], _dump(value))
        else:
            raise ParseError(f"cannot set {key!r} in a scalar")
        return self.src != before

    def delete(self, *path: str) -> bool:
        """Remove path. Returns True when a key was actually removed."""
        if not path:
            raise ValueError("empty path")
        node = self.node(*path)
        if node is None:
            return False
        parent = self.node(*path[:-1])
        key = path[-1]
        changed = False
        if isinstance(parent, Obj):
            member = next(m for m in parent.members if m.key == key)
            start, end = member.key_start, member.value.end
            tail = self.src[end : parent.end - 1]
            comma = tail.find(",")
            if comma >= 0:
                end += comma + 1
            else:
                head = self.src[parent.start + 1 : start]
                prev = head.rfind(",")
                if prev >= 0:
                    start = parent.start + 1 + prev
            while start > 0 and self.src[start - 1] in " \t":
                start -= 1
            changed = True
        elif isinstance(parent, Arr):
            idx = int(key)
            item = parent.items[idx]
            start, end = item.start, item.end
            tail = self.src[end : parent.end - 1]
            comma = tail.find(",")
            if comma >= 0:
                end += comma + 1
            else:
                head = self.src[parent.start + 1 : start]
                prev = head.rfind(",")
                if prev >= 0:
                    start = parent.start + 1 + prev
            changed = True
        if changed:
            self.src = self.src[:start] + self.src[end:]
            self.parse()
        return changed

    def save(self, *, if_changed: bool = True) -> bool:
        """Atomically write back. Returns True when a write happened."""
        if if_changed and self.path.exists():
            current = self.path.read_bytes()
            current = current[3:] if current.startswith(b"\xef\xbb\xbf") else current
            if current.decode("utf-8") == self.src:
                return False
        data = self.src.encode("utf-8")
        if self.bom:
            data = b"\xef\xbb\xbf" + data
        tmp = self.path.with_name(self.path.name + ".tmp")
        tmp.write_bytes(data)
        os.replace(tmp, self.path)
        return True
