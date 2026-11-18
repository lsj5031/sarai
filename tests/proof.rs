//! End-to-end proof with the real Codex and Droid CLIs.
//!
//! Run with:
//! `cargo test --release --test proof -- --ignored --nocapture`
//!
//! The test starts a fake OpenAI-compatible gateway in this process, points an
//! isolated home at it with Sarai, runs each CLI non-interactively, and checks
//! that the request carried our key and model id. It needs `codex` and `droid`
//! on PATH and port 18765 free.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

const KEY: &str = "PROOF-KEY-123";
const MODEL: &str = "proof/echo-model";
const ANSWER: &str = "PROOF-OK";
const PORT: u16 = 18765;

#[derive(Debug)]
struct Hit {
    method: String,
    path: String,
    authorization: Option<String>,
    api_key: Option<String>,
    model: String,
}

fn handle(mut stream: TcpStream, log: Arc<Mutex<Vec<Hit>>>) {
    let Ok(sniff) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(sniff);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    if headers
        .get("expect")
        .is_some_and(|v| v.eq_ignore_ascii_case("100-continue"))
    {
        let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let model = payload["model"].as_str().unwrap_or(MODEL).to_owned();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("").to_owned();
    log.lock().unwrap().push(Hit {
        method: method.clone(),
        path: path.clone(),
        authorization: headers.get("authorization").cloned(),
        api_key: headers.get("x-api-key").cloned(),
        model: model.clone(),
    });

    let route = path.split('?').next().unwrap_or("");
    if method == "GET" && route.starts_with("/v1/models") {
        respond(
            &mut stream,
            200,
            "application/json",
            &json!({"object": "list", "data": [
                {"id": MODEL, "object": "model", "owned_by": "mygw"}
            ]})
            .to_string(),
        );
        return;
    }
    if method != "POST" {
        respond(
            &mut stream,
            404,
            "application/json",
            "{\"error\":\"not found\"}",
        );
        return;
    }
    let streamed = payload["stream"].as_bool().unwrap_or(false);

    if route.starts_with("/v1/responses") {
        let item = json!({
            "id": "msg_proof", "type": "message", "status": "completed",
            "role": "assistant",
            "content": [{"type": "output_text", "text": ANSWER, "annotations": []}]
        });
        let base = json!({
            "id": "resp_proof", "object": "response", "created_at": 1,
            "model": model, "output": []
        });
        let mut done = base.clone();
        done["status"] = json!("completed");
        done["output"] = json!([item.clone()]);
        done["usage"] = json!({"input_tokens": 1, "output_tokens": 1, "total_tokens": 2});
        if streamed {
            send_sse(
                &mut stream,
                &[
                    (
                        "response.created",
                        json!({"type": "response.created", "response": base.clone()}),
                    ),
                    (
                        "response.in_progress",
                        json!({"type": "response.in_progress", "response": base.clone()}),
                    ),
                    (
                        "response.output_item.added",
                        json!({"type": "response.output_item.added", "output_index": 0, "item": item.clone()}),
                    ),
                    (
                        "response.output_text.delta",
                        json!({"type": "response.output_text.delta", "output_index": 0, "content_index": 0, "delta": ANSWER}),
                    ),
                    (
                        "response.output_text.done",
                        json!({"type": "response.output_text.done", "output_index": 0, "content_index": 0, "text": ANSWER}),
                    ),
                    (
                        "response.output_item.done",
                        json!({"type": "response.output_item.done", "output_index": 0, "item": item.clone()}),
                    ),
                    (
                        "response.completed",
                        json!({"type": "response.completed", "response": done}),
                    ),
                ],
            );
        } else {
            respond(&mut stream, 200, "application/json", &done.to_string());
        }
        return;
    }

    if route.starts_with("/v1/chat/completions") {
        if streamed {
            send_sse(
                &mut stream,
                &[
                    (
                        "message",
                        json!({
                            "id": "chatcmpl-proof", "object": "chat.completion.chunk",
                            "created": 1, "model": model,
                            "choices": [{"index": 0, "delta": {"content": ANSWER}, "finish_reason": null}]
                        }),
                    ),
                    (
                        "message",
                        json!({
                            "id": "chatcmpl-proof", "object": "chat.completion.chunk",
                            "created": 1, "model": model,
                            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
                        }),
                    ),
                ],
            );
        } else {
            respond(
                &mut stream,
                200,
                "application/json",
                &json!({
                    "id": "chatcmpl-proof", "object": "chat.completion", "created": 1,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": ANSWER},
                        "finish_reason": "stop"
                    }]
                })
                .to_string(),
            );
        }
        return;
    }

    if route.starts_with("/v1/messages") {
        respond(
            &mut stream,
            200,
            "application/json",
            &json!({
                "id": "msg_proof", "type": "message", "role": "assistant",
                "model": model,
                "content": [{"type": "text", "text": ANSWER}],
                "stop_reason": "end_turn"
            })
            .to_string(),
        );
        return;
    }

    respond(
        &mut stream,
        404,
        "application/json",
        "{\"error\":\"not found\"}",
    );
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
    let head = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn send_sse(stream: &mut TcpStream, events: &[(&str, Value)]) {
    let mut body = String::new();
    for (name, data) in events {
        body.push_str("event: ");
        body.push_str(name);
        body.push_str("\ndata: ");
        body.push_str(&data.to_string());
        body.push_str("\n\n");
    }
    body.push_str("data: [DONE]\n\n");
    respond(stream, 200, "text/event-stream", &body);
}

fn text(path: &std::path::Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[test]
#[ignore = "requires the real codex and droid CLIs"]
fn real_clis_reach_our_gateway() {
    let listener = TcpListener::bind(("127.0.0.1", PORT)).expect("proof gateway port is free");
    let hits = Arc::new(Mutex::new(Vec::new()));
    let server_log = hits.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let log = server_log.clone();
            thread::spawn(move || handle(stream, log));
        }
    });

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let codex_home = home.join("codex");
    let factory_home = home.join("factory");
    fs::create_dir_all(&home).unwrap();

    let def = concat!(env!("CARGO_MANIFEST_DIR"), "/proof/proof_definition.toml");
    let injected = Command::new(env!("CARGO_BIN_EXE_sarai"))
        .arg("-d")
        .arg(def)
        .args(["-a", "codex,droid", "--no-stash"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CODEX_HOME", &codex_home)
        .env("FACTORY_HOME_OVERRIDE", &factory_home)
        .env("NO_COLOR", "1")
        .output()
        .expect("run sarai");
    assert!(
        injected.status.success(),
        "sarai failed:\n{}",
        String::from_utf8_lossy(&injected.stderr)
    );

    println!(
        "--- injected Codex config ---\n{}",
        text(&codex_home.join("config.toml"))
    );
    println!(
        "--- injected Droid settings ---\n{}",
        text(&factory_home.join(".factory/settings.json"))
    );

    let run_cli = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .current_dir(&home)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("CODEX_HOME", &codex_home)
            .env("FACTORY_HOME_OVERRIDE", &factory_home)
            .env("NO_COLOR", "1")
            .output()
            .unwrap_or_else(|e| panic!("running {program} (install it or skip this test): {e}"))
    };

    let codex = run_cli(
        "codex",
        &[
            "exec",
            "--skip-git-repo-check",
            "--dangerously-bypass-approvals-and-sandbox",
            "Reply with exactly PROOF-OK",
        ],
    );
    println!(
        "codex exit: {:?}\n{}",
        codex.status.code(),
        String::from_utf8_lossy(&codex.stdout)
    );
    assert!(
        codex.status.success(),
        "codex exec failed:\n{}",
        String::from_utf8_lossy(&codex.stderr)
    );
    assert!(
        String::from_utf8_lossy(&codex.stdout).contains(ANSWER),
        "codex did not print {ANSWER}"
    );

    let droid = run_cli(
        "droid",
        &["exec", "--auto", "low", "Reply with exactly PROOF-OK"],
    );
    println!(
        "droid exit: {:?}\n{}",
        droid.status.code(),
        String::from_utf8_lossy(&droid.stdout)
    );
    assert!(
        droid.status.success(),
        "droid exec failed:\n{}",
        String::from_utf8_lossy(&droid.stderr)
    );
    assert!(
        String::from_utf8_lossy(&droid.stdout).contains(ANSWER),
        "droid did not print {ANSWER}"
    );

    let hits = hits.lock().unwrap();
    for hit in hits.iter() {
        println!(
            "{} {}  auth={}  model={}",
            hit.method,
            hit.path,
            hit.authorization
                .as_deref()
                .or(hit.api_key.as_deref())
                .unwrap_or("-"),
            hit.model
        );
    }
    let posts: Vec<&Hit> = hits.iter().filter(|h| h.method == "POST").collect();
    assert!(!posts.is_empty(), "the gateway received no POST requests");
    let paths: Vec<&str> = posts
        .iter()
        .map(|h| h.path.split('?').next().unwrap_or(""))
        .collect();
    assert!(
        paths.iter().any(|p| p.ends_with("/v1/responses")),
        "no Responses request: {paths:?}"
    );
    assert!(
        paths.iter().any(|p| p.ends_with("/v1/chat/completions")),
        "no Chat Completions request: {paths:?}"
    );
    for hit in &posts {
        let auth = format!(
            "{}{}",
            hit.authorization.as_deref().unwrap_or(""),
            hit.api_key.as_deref().unwrap_or("")
        );
        assert!(auth.contains(KEY), "our API key did not reach {}", hit.path);
        assert_eq!(hit.model, MODEL, "wrong model at {}", hit.path);
    }
}
