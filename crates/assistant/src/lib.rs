//! The AI coach: answers questions about a build with Claude, grounded in
//! Path of Building.
//!
//! Grounding is structural, not a request to the model:
//! - the build's numbers reach the model only as PoB's output (the overview,
//!   diagnosis and build details) and from tools that run PoB ([`tools`]);
//! - item, gem and mechanic facts come from PoB's game data or from searches
//!   of a few allowed sites, cited;
//! - every number in an answer is checked against what the model was given
//!   ([`grounding`]), and the ones that match nothing are returned for the
//!   app to mark.
//!
//! A [`Conversation`] keeps the messages for one build. Requests go to the
//! Claude API with the player's own key ([`api`]); each answer reports its
//! token use and cost ([`price`]).

pub mod api;
pub mod grounding;
pub mod price;
pub mod prompt;
pub mod tools;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use serde::Serialize;
use serde_json::{Value, json};

pub use api::{Client, Error, Usage};
pub use tools::Bench;

/// Requests one question may take (each tool round is one).
const MAX_REQUESTS: usize = 16;
const MAX_TOKENS: u32 = 64_000;

/// A page an answer cites.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Source {
    pub title: String,
    pub url: String,
    /// The passage cited, as the API returned it.
    pub quote: String,
}

/// One answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    /// Markdown, with `[n]` after text drawn from `sources[n-1]`.
    pub text: String,
    pub sources: Vec<Source>,
    /// Numbers in `text` that match nothing the coach was given, as written.
    pub unverified: Vec<String>,
    /// How many numbers in `text` were checked.
    pub numbers_checked: usize,
    /// What the coach checked, in order ("Tested in PoB: …").
    pub steps: Vec<String>,
    pub usage: Usage,
    /// US dollars, when the model's prices are known.
    pub cost: Option<f64>,
    pub model: String,
    /// Why the answer is incomplete or unusual, if it is.
    pub note: Option<String>,
}

/// What happens while an answer is worked out.
#[derive(Debug, Clone)]
pub enum Progress {
    Text(String),
    Step(String),
}

/// A conversation about one build.
#[derive(Debug)]
pub struct Conversation {
    context: String,
    messages: Vec<Value>,
    evidence: grounding::Evidence,
    /// Fetched pages' titles to their addresses, for citations.
    documents: HashMap<String, String>,
    slot_cache: tools::SlotCache,
}

impl Conversation {
    /// A new conversation about the build described by `context` (see
    /// [`prompt::context`]).
    pub fn new(context: String) -> Self {
        let mut evidence = grounding::Evidence::default();
        evidence.add_text(&context);
        Self {
            context,
            messages: Vec::new(),
            evidence,
            documents: HashMap::new(),
            slot_cache: HashMap::new(),
        }
    }

    pub fn questions(&self) -> usize {
        self.messages
            .iter()
            .filter(|m| m["role"] == "user" && !is_tool_results(m))
            .count()
    }

    /// Asks `question`. On an error nothing of the question is kept, so the
    /// conversation can continue.
    pub fn ask(
        &mut self,
        client: &Client,
        model: &str,
        bench: Bench<'_>,
        question: &str,
        cancel: &AtomicBool,
        on: &mut dyn FnMut(Progress),
    ) -> Result<Answer, Error> {
        let start = self.messages.len();
        let evidence = self.evidence.clone();
        let result = self.work(client, model, bench, question, cancel, on);
        let keep = matches!(&result, Ok((_, true)));
        if !keep {
            self.messages.truncate(start);
            self.evidence = evidence;
        }
        result.map(|(answer, _)| answer)
    }

    /// The answer, and whether the conversation can continue from it.
    fn work(
        &mut self,
        client: &Client,
        model: &str,
        bench: Bench<'_>,
        question: &str,
        cancel: &AtomicBool,
        on: &mut dyn FnMut(Progress),
    ) -> Result<(Answer, bool), Error> {
        self.evidence.add_text(question);
        self.messages.push(if self.messages.is_empty() {
            prompt::first_message(&self.context, question)
        } else {
            json!({"role": "user", "content": [{"type": "text", "text": question}]})
        });

        let mut written = Written::default();
        let mut steps = Vec::new();
        let mut usage = Usage::default();
        let mut cost = Some(0.0);
        let mut served_by = model.to_owned();
        let mut note = None;
        let mut complete = false;

        for _ in 0..MAX_REQUESTS {
            let body = request(model, &self.messages);
            let reply = client.stream(&body, cancel, &mut |streamed| match streamed {
                api::Streamed::Text(text) => on(Progress::Text(text.to_owned())),
                api::Streamed::Block(block) => {
                    if let Some(step) = step(block) {
                        on(Progress::Step(step));
                    }
                }
            })?;
            usage.add(&reply.usage);
            if !reply.model.is_empty() {
                served_by = reply.model.clone();
            }
            cost = cost
                .zip(price::cost(&served_by, &reply.usage))
                .map(|(a, b)| a + b);
            steps.extend(reply.content.iter().filter_map(step));
            if let Some(fallback) = reply.content.iter().find(|b| b["type"] == "fallback") {
                let to = fallback.pointer("/to/model").and_then(Value::as_str);
                note = Some(format!(
                    "The first model declined; {} answered instead.",
                    to.unwrap_or("another model")
                ));
                cost = None;
            }
            self.read_reply(&reply.content, &mut written);
            let calls: Vec<&Value> = reply
                .content
                .iter()
                .filter(|b| b["type"] == "tool_use")
                .collect();
            let results = (reply.stop_reason == "tool_use").then(|| {
                calls
                    .iter()
                    .map(|call| self.run_tool(bench, call, &reply.invalid_inputs))
                    .collect::<Vec<_>>()
            });
            let pending = !calls.is_empty() && results.is_none();
            self.messages
                .push(json!({"role": "assistant", "content": reply.content}));
            match reply.stop_reason.as_str() {
                "tool_use" => {
                    let results = results.unwrap_or_default();
                    for result in &results {
                        if let Some(text) = result["content"].as_str() {
                            self.evidence.add_text(text);
                        }
                    }
                    self.messages
                        .push(json!({"role": "user", "content": results}));
                }
                "pause_turn" => {}
                "end_turn" | "stop_sequence" => {
                    complete = true;
                    break;
                }
                other => {
                    note = Some(
                        match other {
                            "max_tokens" => "The answer reached its length limit and is cut short.",
                            "refusal" => "Claude declined to answer this.",
                            "model_context_window_exceeded" => {
                                "The conversation is too long. Start a new one."
                            }
                            _ => "The answer stopped early.",
                        }
                        .to_owned(),
                    );
                    complete = !pending;
                    break;
                }
            }
        }
        if note.is_none() && !complete {
            note = Some(
                "The coach needed too many steps and stopped. Ask a narrower question.".into(),
            );
        }
        let continues = complete
            || self
                .messages
                .last()
                .is_some_and(|m| m["role"] == "assistant");
        let check = self.evidence.check(&written.plain);
        Ok((
            Answer {
                text: written.text.trim().to_owned(),
                sources: written.sources,
                unverified: check.unverified,
                numbers_checked: check.numbers,
                steps,
                usage,
                cost,
                model: served_by,
                note,
            },
            continues,
        ))
    }

    /// Collects the answer text, citations and anything it may quote.
    fn read_reply(&mut self, content: &[Value], written: &mut Written) {
        let mut started = false;
        for block in content {
            match block["type"].as_str().unwrap_or("") {
                "text" => {
                    let text = block["text"].as_str().unwrap_or("");
                    if !started && !written.text.is_empty() {
                        written.text.push_str("\n\n");
                        written.plain.push_str("\n\n");
                    }
                    started = true;
                    written.text.push_str(text);
                    written.plain.push_str(text);
                    let mut marks = Vec::new();
                    for citation in block["citations"].as_array().into_iter().flatten() {
                        let quote = citation["cited_text"].as_str().unwrap_or("").to_owned();
                        self.evidence.add_text(&quote);
                        let title = citation
                            .get("title")
                            .or_else(|| citation.get("document_title"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        let url = citation["url"]
                            .as_str()
                            .map(str::to_owned)
                            .or_else(|| self.documents.get(&title).cloned())
                            .unwrap_or_default();
                        let source = Source { title, url, quote };
                        let n = match written
                            .sources
                            .iter()
                            .position(|s| s.url == source.url && !s.url.is_empty())
                        {
                            Some(i) => i + 1,
                            None => {
                                written.sources.push(source);
                                written.sources.len()
                            }
                        };
                        if !marks.contains(&n) {
                            marks.push(n);
                        }
                    }
                    for n in marks {
                        written.text.push_str(&format!(" [{n}]"));
                    }
                }
                "tool_use" | "server_tool_use" => {
                    self.evidence.add_text(&block["input"].to_string());
                }
                "web_fetch_tool_result" => {
                    let result = &block["content"];
                    let url = result["url"].as_str().unwrap_or("");
                    let title = result.pointer("/content/title").and_then(Value::as_str);
                    if let Some(title) = title {
                        self.documents.insert(title.to_owned(), url.to_owned());
                    }
                    if result.pointer("/content/source/type") == Some(&json!("text"))
                        && let Some(page) = result
                            .pointer("/content/source/data")
                            .and_then(Value::as_str)
                    {
                        self.evidence.add_text(page);
                    }
                }
                _ => {}
            }
        }
    }

    fn run_tool(&mut self, bench: Bench<'_>, call: &Value, invalid: &[(String, String)]) -> Value {
        let id = call["id"].as_str().unwrap_or("");
        let name = call["name"].as_str().unwrap_or("");
        let outcome = match invalid.iter().find(|(bad, _)| bad == id) {
            Some((_, raw)) => tools::Outcome {
                content: json!({"INVALID_JSON": raw}).to_string(),
                is_error: true,
            },
            None => tools::run(bench, &mut self.slot_cache, name, &call["input"]),
        };
        json!({
            "type": "tool_result",
            "tool_use_id": id,
            "content": outcome.content,
            "is_error": outcome.is_error,
        })
    }
}

/// The build data a conversation starts from: PoB's overview, diagnosis,
/// brief and realistic numbers as given, plus what the build wears, sockets
/// and allocates, read from PoB.
pub fn build_context(
    bench: Bench<'_>,
    overview: &analysis::Overview,
    diagnosis: Option<&analysis::Diagnosis>,
    brief: Option<&analysis::Brief>,
    realistic: Option<&analysis::Realistic>,
) -> Result<String, String> {
    let details = tools::on_engine(bench, |engine| engine.build_details())?;
    Ok(prompt::context(
        &bench.pool.install().version().to_string(),
        &today(),
        &to_json(overview),
        diagnosis.map(to_json).as_ref(),
        brief.map(to_json).as_ref(),
        realistic.map(to_json).as_ref(),
        &details,
    ))
}

fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// The answer as it is assembled.
#[derive(Default)]
struct Written {
    /// With citation marks.
    text: String,
    /// Without them, for the number check.
    plain: String,
    sources: Vec<Source>,
}

fn is_tool_results(message: &Value) -> bool {
    message["content"]
        .as_array()
        .is_some_and(|c| !c.is_empty() && c.iter().all(|b| b["type"] == "tool_result"))
}

/// The step a finished block stands for, if it is a tool call.
fn step(block: &Value) -> Option<String> {
    match block["type"].as_str()? {
        "tool_use" | "server_tool_use" => {
            Some(tools::describe(block["name"].as_str()?, &block["input"]))
        }
        _ => None,
    }
}

/// A streamed Messages request. The system prompt and tools never change, so
/// with automatic caching each request reads the conversation so far from
/// the cache.
fn request(model: &str, messages: &[Value]) -> Value {
    let mut tools = tools::definitions();
    tools.extend(tools::server_tools());
    json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "thinking": {"type": "adaptive"},
        "output_config": {"effort": "high"},
        "cache_control": {"type": "ephemeral"},
        "fallbacks": "default",
        "system": [{"type": "text", "text": prompt::SYSTEM}],
        "tools": tools,
        "messages": messages,
    })
}

/// Today's date (UTC) as YYYY-MM-DD.
pub fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_777), (2026, 11, 20));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn request_has_the_grounding_tools_and_caching() {
        let body = request(
            "claude-opus-5-5",
            &[json!({"role": "user", "content": "hi"})],
        );
        assert_eq!(body["stream"], true);
        assert_eq!(body["cache_control"]["type"], "ephemeral");
        assert_eq!(body["thinking"]["type"], "adaptive");
        let names: Vec<&str> = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"what_if_modifiers"));
        assert!(names.contains(&"web_search"));
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn citations_become_numbered_sources() {
        let mut conversation = Conversation::new("FireResist 68".into());
        let mut written = Written::default();
        conversation.read_reply(
            &[
                json!({"type": "text", "text": "Your fire resistance is 68%."}),
                json!({"type": "text", "text": "The cap is 75%.", "citations": [
                    {"type": "web_search_result_location", "url": "https://www.poe2wiki.net/wiki/Resistance", "title": "Resistance", "cited_text": "maximum of 75%"}
                ]}),
                json!({"type": "web_fetch_tool_result", "content": {"type": "web_fetch_result", "url": "https://maxroll.gg/poe2/x", "content": {"type": "document", "title": "Guide", "source": {"type": "text", "data": "Armour 5000"}}}}),
            ],
            &mut written,
        );
        conversation.read_reply(
            &[json!({"type": "text", "text": "Armour helps.", "citations": [
                {"type": "char_location", "document_title": "Guide", "cited_text": "Armour 5000"}
            ]})],
            &mut written,
        );
        assert_eq!(
            written.text,
            "Your fire resistance is 68%.The cap is 75%. [1]\n\nArmour helps. [2]"
        );
        assert_eq!(written.sources[1].url, "https://maxroll.gg/poe2/x");
        assert!(
            conversation
                .evidence
                .unverified("68% and 75% and 5000 armour")
                .is_empty()
        );
        assert_eq!(conversation.evidence.unverified("90% chaos"), vec!["90%"]);
    }
}
