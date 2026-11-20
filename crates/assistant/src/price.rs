//! What an answer cost, from the token counts the API reported.
//!
//! Rates are Anthropic's list prices in US dollars per million tokens, from
//! <https://platform.claude.com/docs/en/about-claude/pricing> (checked
//! 2026-11-20): 5-minute cache writes, cache reads, and web search at $10
//! per 1,000 searches. A model not listed here has no cost shown.

use crate::api::Usage;

/// The models the coach offers, best first: (API id, name, note).
pub const MODELS: &[(&str, &str, &str)] = &[
    (
        "claude-opus-5-5",
        "Claude Opus 5.5",
        "Most careful reasoning",
    ),
    (
        "claude-sonnet-5-5",
        "Claude Sonnet 5.5",
        "About half the cost",
    ),
];

struct Rates {
    input: f64,
    cache_write: f64,
    cache_read: f64,
    output: f64,
}

const WEB_SEARCH: f64 = 10.0 / 1000.0;

fn rates(model: &str) -> Option<Rates> {
    match model {
        "claude-opus-5-5" => Some(Rates {
            input: 4.0,
            cache_write: 5.0,
            cache_read: 0.20,
            output: 20.0,
        }),
        "claude-sonnet-5-5" => Some(Rates {
            input: 2.0,
            cache_write: 2.50,
            cache_read: 0.10,
            output: 10.0,
        }),
        _ => None,
    }
}

/// The cost of `usage` on `model` in US dollars, if its prices are known.
pub fn cost(model: &str, usage: &Usage) -> Option<f64> {
    let r = rates(model)?;
    let tokens = |count: u64, rate: f64| count as f64 * rate / 1e6;
    Some(
        tokens(usage.input, r.input)
            + tokens(usage.cache_write, r.cache_write)
            + tokens(usage.cache_read, r.cache_read)
            + tokens(usage.output, r.output)
            + usage.web_searches as f64 * WEB_SEARCH,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_each_kind_of_token() {
        let usage = Usage {
            input: 1_000_000,
            cache_write: 1_000_000,
            cache_read: 1_000_000,
            output: 1_000_000,
            web_searches: 100,
        };
        let opus = cost("claude-opus-5-5", &usage).unwrap();
        assert!((opus - (4.0 + 5.0 + 0.2 + 20.0 + 1.0)).abs() < 1e-9);
        let sonnet = cost("claude-sonnet-5-5", &usage).unwrap();
        assert!((sonnet - (2.0 + 2.5 + 0.1 + 10.0 + 1.0)).abs() < 1e-9);
        assert_eq!(cost("some-other-model", &usage), None);
    }
}
