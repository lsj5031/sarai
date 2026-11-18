"""Tiny fake OpenAI-compatible endpoint for the end-to-end proof.

Logs every request (path, auth header, body) to proof/gateway.log and returns
just enough of a Responses / Chat Completions reply for the real CLIs to
finish. Nothing is sent anywhere else.
"""

from __future__ import annotations

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

PORT = 18765
MODEL = "proof/echo-model"
ANSWER = "PROOF-OK"
LOG = Path(__file__).with_name("gateway.log")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_):
        pass

    def record(self, body: str, model: str = "") -> None:
        with LOG.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps({
                "method": self.command,
                "path": self.path,
                "authorization": self.headers.get("Authorization"),
                "api_key": self.headers.get("x-api-key"),
                "model": model,
                "body": body[:1500],
            }) + "\n")

    def send_json(self, obj, status: int = 200) -> None:
        data = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def send_sse(self, events: list[tuple[str, dict]]) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()
        for name, data in events:
            self.wfile.write(f"event: {name}\ndata: {json.dumps(data)}\n\n".encode())
        self.wfile.write(b"data: [DONE]\n\n")

    def do_GET(self):
        self.record("", MODEL)
        if self.path.startswith("/v1/models"):
            self.send_json({"object": "list", "data": [
                {"id": MODEL, "object": "model", "owned_by": "mygw"}
            ]})
        else:
            self.send_json({"error": "not found"}, 404)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0") or 0)
        body = self.rfile.read(length).decode("utf-8", "replace")
        try:
            payload = json.loads(body)
        except json.JSONDecodeError:
            payload = {}
        model = payload.get("model", MODEL)
        self.record(body, model)
        stream = bool(payload.get("stream"))

        if self.path.startswith("/v1/responses"):
            item = {"id": "msg_proof", "type": "message", "status": "completed",
                    "role": "assistant", "content": [
                        {"type": "output_text", "text": ANSWER, "annotations": []}]}
            base = {"id": "resp_proof", "object": "response", "created_at": 1,
                    "model": model, "output": []}
            if stream:
                self.send_sse([
                    ("response.created", {"type": "response.created", "response": base}),
                    ("response.in_progress", {"type": "response.in_progress", "response": base}),
                    ("response.output_item.added", {"type": "response.output_item.added",
                                                    "output_index": 0, "item": item}),
                    ("response.output_text.delta", {"type": "response.output_text.delta",
                                                    "output_index": 0, "content_index": 0,
                                                    "delta": ANSWER}),
                    ("response.output_text.done", {"type": "response.output_text.done",
                                                   "output_index": 0, "content_index": 0,
                                                   "text": ANSWER}),
                    ("response.output_item.done", {"type": "response.output_item.done",
                                                   "output_index": 0, "item": item}),
                    ("response.completed", {"type": "response.completed",
                                            "response": {**base, "status": "completed",
                                                         "output": [item],
                                                         "usage": {"input_tokens": 1,
                                                                   "output_tokens": 1,
                                                                   "total_tokens": 2}}}),
                ])
            else:
                self.send_json({**base, "status": "completed", "output": [item]})
            return

        if self.path.startswith("/v1/chat/completions"):
            if stream:
                self.send_sse([
                    ("message", {"id": "chatcmpl-proof", "object": "chat.completion.chunk",
                                 "created": 1, "model": model,
                                 "choices": [{"index": 0, "delta": {"content": ANSWER},
                                              "finish_reason": None}]}),
                    ("message", {"id": "chatcmpl-proof", "object": "chat.completion.chunk",
                                 "created": 1, "model": model,
                                 "choices": [{"index": 0, "delta": {},
                                              "finish_reason": "stop"}]}),
                ])
            else:
                self.send_json({"id": "chatcmpl-proof", "object": "chat.completion",
                                "created": 1, "model": model, "choices": [{
                                    "index": 0,
                                    "message": {"role": "assistant", "content": ANSWER},
                                    "finish_reason": "stop"}]})
            return

        if self.path.startswith("/v1/messages"):
            self.send_json({"id": "msg_proof", "type": "message", "role": "assistant",
                            "model": model, "content": [{"type": "text", "text": ANSWER}],
                            "stop_reason": "end_turn"})
            return

        self.send_json({"error": "not found"}, 404)


if __name__ == "__main__":
    LOG.write_text("", encoding="utf-8")
    print(f"fake gateway on http://127.0.0.1:{PORT}/v1", flush=True)
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
