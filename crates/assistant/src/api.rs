//! The Claude Messages API over plain HTTP: Rust has no official Anthropic
//! SDK, so requests are JSON and replies are read as a server-sent event
//! stream. Every content block is kept exactly as received (thinking blocks,
//! search results, citations), because the API needs them back unchanged on
//! the next request.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Map, Value, json};

const API_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2026-10-21";
/// Lets the API rerun a request on another model when the first declines
/// it (`fallbacks: "default"` in the request).
const BETAS: &str = "server-side-fallback-2026-07-01";
/// Attempts for a request the API asks to retry (rate limit, overload).
const ATTEMPTS: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("The API key was not accepted. Check it under Settings.")]
    Key,
    #[error("The account is out of credit or over its spending limit ({0}).")]
    Billing(String),
    #[error("Anthropic's API is busy right now. Try again in a minute.")]
    Busy,
    #[error("Anthropic's API refused the request: {0}")]
    Api(String),
    #[error("Could not reach Anthropic's API. Check your internet connection. ({0})")]
    Network(String),
    #[error("The answer was stopped.")]
    Cancelled,
}

/// Tokens and searches one request used, as the API reported them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub output: u64,
    pub web_searches: u64,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.input += other.input;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
        self.output += other.output;
        self.web_searches += other.web_searches;
    }

    /// Takes the fields `usage` reports; streamed totals only grow.
    fn update(&mut self, usage: &Value) {
        let read = |key: &str| usage.get(key).and_then(Value::as_u64);
        if let Some(v) = read("input_tokens") {
            self.input = v;
        }
        if let Some(v) = read("cache_creation_input_tokens") {
            self.cache_write = v;
        }
        if let Some(v) = read("cache_read_input_tokens") {
            self.cache_read = v;
        }
        if let Some(v) = read("output_tokens") {
            self.output = v;
        }
        if let Some(v) = usage
            .pointer("/server_tool_use/web_search_requests")
            .and_then(Value::as_u64)
        {
            self.web_searches = v;
        }
    }
}

/// One complete reply.
#[derive(Debug, Clone)]
pub struct Reply {
    /// The content blocks, as the API sent them.
    pub content: Vec<Value>,
    pub stop_reason: String,
    pub usage: Usage,
    /// The model that answered.
    pub model: String,
    /// Tool calls whose input was not valid JSON, by tool use id, with the
    /// raw text received.
    pub invalid_inputs: Vec<(String, String)>,
}

/// What arrives while a reply streams.
#[derive(Debug)]
pub enum Streamed<'a> {
    /// More answer text.
    Text(&'a str),
    /// A finished content block (a tool call, a search, a result).
    Block(&'a Value),
}

#[derive(Debug, Clone)]
pub struct Client {
    agent: ureq::Agent,
    key: String,
    base: String,
}

impl Client {
    /// `base` replaces Anthropic's API address (for tests against a local
    /// server).
    pub fn new(key: &str, base: Option<&str>) -> Self {
        let agent = ureq::Agent::config_builder()
            .user_agent(concat!("build-doctor/", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(180)))
            .build()
            .into();
        Self {
            agent,
            key: key.trim().to_owned(),
            base: base.unwrap_or(API_URL).trim_end_matches('/').to_owned(),
        }
    }

    /// Sends `body` (a Messages request with `stream: true`) and reads the
    /// reply as it streams. Rate limits and overloads are retried before
    /// anything has streamed.
    pub fn stream(
        &self,
        body: &Value,
        cancel: &AtomicBool,
        on: &mut dyn FnMut(Streamed<'_>),
    ) -> Result<Reply, Error> {
        let payload = serde_json::to_vec(body).map_err(|e| Error::Api(e.to_string()))?;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let response = self
                .agent
                .post(&format!("{}/v1/messages", self.base))
                .header("x-api-key", &self.key)
                .header("anthropic-version", API_VERSION)
                .header("anthropic-beta", BETAS)
                .header("content-type", "application/json")
                .send(&payload[..])
                .map_err(|e| Error::Network(e.to_string()))?;
            let status = response.status().as_u16();
            if status == 200 {
                return read_stream(response.into_body().into_reader(), cancel, on);
            }
            let wait = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let text = response.into_body().read_to_string().unwrap_or_default();
            let error = api_error(status, &text);
            let retry = matches!(status, 429 | 500 | 502 | 503 | 529);
            if !retry || attempt >= ATTEMPTS {
                return Err(error);
            }
            let seconds = wait.unwrap_or(2u64.pow(attempt)).min(30);
            for _ in 0..seconds * 10 {
                if cancel.load(Ordering::SeqCst) {
                    return Err(Error::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// A plain error for an HTTP failure, from the API's error body.
fn api_error(status: u16, body: &str) -> Error {
    let message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.pointer("/error/message")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("HTTP {status}"));
    match status {
        401 | 403 => Error::Key,
        402 => Error::Billing(message),
        400 if message.contains("credit balance") => Error::Billing(message),
        429 | 500 | 502 | 503 | 529 => Error::Busy,
        _ => Error::Api(message),
    }
}

/// The content blocks of a reply being assembled from stream events.
#[derive(Default)]
struct Assembly {
    blocks: Vec<Value>,
    /// Tool input JSON as it streams, per block.
    json: Vec<String>,
    stop_reason: Option<String>,
    usage: Usage,
    model: String,
    invalid_inputs: Vec<(String, String)>,
}

fn read_stream(
    reader: impl std::io::Read,
    cancel: &AtomicBool,
    on: &mut dyn FnMut(Streamed<'_>),
) -> Result<Reply, Error> {
    let mut assembly = Assembly::default();
    let mut data = String::new();
    let mut done = false;
    for line in BufReader::new(reader).lines() {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::Cancelled);
        }
        let line = line.map_err(|e| Error::Network(e.to_string()))?;
        if line.is_empty() {
            if !data.is_empty() {
                done = assembly.event(&data, on)?;
                data.clear();
            }
            if done {
                break;
            }
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if !data.is_empty() && !done {
        done = assembly.event(&data, on)?;
    }
    let stop_reason = assembly
        .stop_reason
        .filter(|_| done)
        .ok_or_else(|| Error::Network("the reply stopped before it was complete".into()))?;
    Ok(Reply {
        content: assembly.blocks,
        stop_reason,
        usage: assembly.usage,
        model: assembly.model,
        invalid_inputs: assembly.invalid_inputs,
    })
}

impl Assembly {
    /// Applies one event; true once the message is complete.
    fn event(&mut self, data: &str, on: &mut dyn FnMut(Streamed<'_>)) -> Result<bool, Error> {
        let event: Value = serde_json::from_str(data)
            .map_err(|e| Error::Network(format!("unreadable stream event: {e}")))?;
        let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => {
                if let Some(message) = event.get("message") {
                    self.model = message
                        .get("model")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    if let Some(usage) = message.get("usage") {
                        self.usage.update(usage);
                    }
                }
            }
            "content_block_start" => {
                while self.blocks.len() <= index {
                    self.blocks.push(Value::Null);
                    self.json.push(String::new());
                }
                self.blocks[index] = event.get("content_block").cloned().unwrap_or(Value::Null);
            }
            "content_block_delta" => self.delta(index, event.get("delta"), on),
            "content_block_stop" => {
                self.finish_block(index);
                if let Some(block) = self.blocks.get(index) {
                    on(Streamed::Block(block));
                }
            }
            "message_delta" => {
                if let Some(reason) = event.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(reason.to_owned());
                }
                if let Some(usage) = event.get("usage") {
                    self.usage.update(usage);
                }
            }
            "message_stop" => return Ok(true),
            "error" => {
                let kind = event.pointer("/error/type").and_then(Value::as_str);
                let message = event
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(match kind {
                    Some("overloaded_error" | "rate_limit_error") => Error::Busy,
                    _ => Error::Api(message.to_owned()),
                });
            }
            _ => {}
        }
        Ok(false)
    }

    fn delta(&mut self, index: usize, delta: Option<&Value>, on: &mut dyn FnMut(Streamed<'_>)) {
        let (Some(delta), Some(block)) = (delta, self.blocks.get_mut(index)) else {
            return;
        };
        let Some(object) = block.as_object_mut() else {
            return;
        };
        let text = |key: &str| delta.get(key).and_then(Value::as_str).unwrap_or("");
        match delta.get("type").and_then(Value::as_str).unwrap_or("") {
            "text_delta" => {
                append(object, "text", text("text"));
                on(Streamed::Text(text("text")));
            }
            "input_json_delta" => self.json[index].push_str(text("partial_json")),
            "thinking_delta" => append(object, "thinking", text("thinking")),
            "signature_delta" => {
                object.insert("signature".into(), Value::String(text("signature").into()));
            }
            "citations_delta" => {
                if let Some(citation) = delta.get("citation") {
                    let list = object
                        .entry("citations")
                        .or_insert_with(|| Value::Array(Vec::new()));
                    if !list.is_array() {
                        *list = Value::Array(Vec::new());
                    }
                    if let Some(list) = list.as_array_mut() {
                        list.push(citation.clone());
                    }
                }
            }
            _ => {}
        }
    }

    /// Parses a finished tool call's input. Input is streamed without the
    /// API's validation (`eager_input_streaming`), so it is parsed strictly
    /// here and a bad one is recorded, never run.
    fn finish_block(&mut self, index: usize) {
        let raw = std::mem::take(&mut self.json[index]);
        let Some(block) = self.blocks.get_mut(index).and_then(Value::as_object_mut) else {
            return;
        };
        let is_call = matches!(
            block.get("type").and_then(Value::as_str),
            Some("tool_use" | "server_tool_use")
        );
        if !is_call || raw.is_empty() {
            return;
        }
        match serde_json::from_str::<Value>(&raw) {
            Ok(input @ Value::Object(_)) => {
                block.insert("input".into(), input);
            }
            _ => {
                let id = block.get("id").and_then(Value::as_str).unwrap_or("");
                self.invalid_inputs.push((id.to_owned(), raw));
                block.insert("input".into(), json!({}));
            }
        }
    }
}

fn append(object: &mut Map<String, Value>, key: &str, more: &str) {
    let entry = object
        .entry(key)
        .or_insert_with(|| Value::String(String::new()));
    if let Value::String(s) = entry {
        s.push_str(more);
    } else {
        *entry = Value::String(more.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sse(events: &[Value]) -> String {
        events
            .iter()
            .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
            .collect()
    }

    #[test]
    fn assembles_text_tool_calls_citations_and_usage() {
        let stream = sse(&[
            json!({"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":10,"cache_read_input_tokens":90}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Fire "}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"res"}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"citations_delta","citation":{"type":"web_search_result_location","url":"https://www.poe2wiki.net/wiki/Resistance","cited_text":"75%"}}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"get_stats","input":{}}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"keys\":[\"Fire"}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"Resist\"]}"}}),
            json!({"type":"content_block_stop","index":2}),
            json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_2","name":"get_stats","input":{}}}),
            json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"keys\":[\"Fi"}}),
            json!({"type":"content_block_stop","index":3}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42,"server_tool_use":{"web_search_requests":1}}}),
            json!({"type":"message_stop"}),
        ]);
        let mut texts = String::new();
        let mut blocks = 0;
        let reply = read_stream(
            stream.as_bytes(),
            &AtomicBool::new(false),
            &mut |s| match s {
                Streamed::Text(t) => texts.push_str(t),
                Streamed::Block(_) => blocks += 1,
            },
        )
        .unwrap();
        assert_eq!(texts, "Fire res");
        assert_eq!(blocks, 4);
        assert_eq!(reply.stop_reason, "tool_use");
        assert_eq!(reply.model, "claude-opus-5-5");
        assert_eq!(reply.content[0]["signature"], "sig");
        assert_eq!(reply.content[1]["text"], "Fire res");
        assert_eq!(reply.content[1]["citations"][0]["cited_text"], "75%");
        assert_eq!(reply.content[2]["input"], json!({"keys": ["FireResist"]}));
        assert_eq!(reply.content[3]["input"], json!({}));
        assert_eq!(
            reply.invalid_inputs,
            vec![("toolu_2".to_owned(), "{\"keys\":[\"Fi".to_owned())]
        );
        assert_eq!(
            reply.usage,
            Usage {
                input: 10,
                cache_write: 0,
                cache_read: 90,
                output: 42,
                web_searches: 1
            }
        );
    }

    #[test]
    fn an_unfinished_stream_is_an_error() {
        let stream = sse(&[
            json!({"type":"message_start","message":{"model":"m","usage":{}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        ]);
        let result = read_stream(stream.as_bytes(), &AtomicBool::new(false), &mut |_| {});
        assert!(matches!(result, Err(Error::Network(_))));
    }

    #[test]
    fn stream_errors_and_http_errors_read_plainly() {
        let stream = sse(&[
            json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}),
        ]);
        let result = read_stream(stream.as_bytes(), &AtomicBool::new(false), &mut |_| {});
        assert!(matches!(result, Err(Error::Busy)));
        assert!(matches!(api_error(401, ""), Error::Key));
        assert!(matches!(
            api_error(
                400,
                r#"{"error":{"message":"Your credit balance is too low"}}"#
            ),
            Error::Billing(_)
        ));
        match api_error(400, r#"{"error":{"message":"bad field"}}"#) {
            Error::Api(m) => assert_eq!(m, "bad field"),
            other => panic!("{other:?}"),
        }
    }
}
