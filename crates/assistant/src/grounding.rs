//! The number check: every number in an answer must be one the coach was
//! given (the build data, a PoB result, the player's own words, a cited
//! passage or a fetched page). Numbers that match none of them are returned
//! so the app can mark them; nothing is hidden or rewritten.

/// A number as written in text, with its value and how precisely it was
/// written.
#[derive(Debug, Clone, PartialEq)]
struct Written {
    text: String,
    value: f64,
    /// Half a unit of the last digit written (0.5 for "12", 0.05 for "1.2",
    /// 50 000 for "1.2M").
    slack: f64,
    percent: bool,
    small_count: bool,
}

/// Every number the coach was given.
#[derive(Debug, Default, Clone)]
pub struct Evidence {
    values: Vec<f64>,
    sorted: bool,
}

impl Evidence {
    pub fn add_text(&mut self, text: &str) {
        for written in numbers(text) {
            let v = written.value.abs();
            self.values.push(v);
            // Fractions PoB keeps (0.28) are read as percentages (28%), and
            // the other way round.
            if v <= 1.0 {
                self.values.push(v * 100.0);
            }
            self.values.push(v / 100.0);
        }
        self.sorted = false;
    }

    fn contains(&mut self, written: &Written) -> bool {
        if !self.sorted {
            self.values.sort_by(f64::total_cmp);
            self.values.dedup();
            self.sorted = true;
        }
        let value = written.value.abs();
        let slack = written.slack.max(value * 0.005);
        let start = self.values.partition_point(|v| *v < value - slack);
        self.values.get(start).is_some_and(|v| *v <= value + slack)
    }

    /// Checks every number in `answer`. Small whole numbers that count
    /// things ("2 rings", "step 3") are not checked.
    pub fn check(&mut self, answer: &str) -> Check {
        let mut check = Check::default();
        for written in numbers(answer) {
            if written.small_count && !written.percent {
                continue;
            }
            check.numbers += 1;
            if !self.contains(&written) && !check.unverified.contains(&written.text) {
                check.unverified.push(written.text);
            }
        }
        check
    }

    /// The numbers in `answer` that match nothing given, as written.
    pub fn unverified(&mut self, answer: &str) -> Vec<String> {
        self.check(answer).unverified
    }
}

/// The result of checking an answer's numbers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Check {
    /// Numbers checked.
    pub numbers: usize,
    /// Those that match nothing given, as written, without repeats.
    pub unverified: Vec<String>,
}

/// The numbers in `text`: digits with optional thousands separators,
/// decimals, a k/M suffix and a percent sign. Digits inside words ("PoE2",
/// "x2") and version numbers ("0.23.1") are not numbers.
fn numbers(text: &str) -> Vec<Written> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let starts = chars[i].is_ascii_digit()
            && (i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '.'));
        if !starts {
            i += 1;
            continue;
        }
        let begin = i;
        let mut digits = String::new();
        let mut decimals: Option<usize> = None;
        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_digit() {
                digits.push(c);
                if let Some(d) = decimals.as_mut() {
                    *d += 1;
                }
                i += 1;
            } else if c == ','
                && decimals.is_none()
                && chars
                    .get(i + 1..i + 4)
                    .is_some_and(|g| g.iter().all(char::is_ascii_digit))
                && !chars.get(i + 4).is_some_and(char::is_ascii_digit)
            {
                i += 1;
            } else if c == '.'
                && decimals.is_none()
                && chars.get(i + 1).is_some_and(char::is_ascii_digit)
            {
                digits.push('.');
                decimals = Some(0);
                i += 1;
            } else {
                break;
            }
        }
        // "0.23.1": a version, not a number.
        if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            continue;
        }
        let mut scale = 1.0;
        let mut end = i;
        if let Some(&suffix) = chars.get(i) {
            let next_is_letter = chars.get(i + 1).is_some_and(|c| c.is_alphabetic());
            if !next_is_letter {
                scale = match suffix {
                    'k' | 'K' => 1e3,
                    'M' => 1e6,
                    _ => 1.0,
                };
                if scale > 1.0 {
                    end = i + 1;
                }
            }
        }
        let percent = chars.get(end) == Some(&'%');
        if percent {
            end += 1;
        }
        // A word right after the digits ("2x", "4th") is not a quantity.
        if scale == 1.0 && !percent && chars.get(end).is_some_and(|c| c.is_alphabetic()) {
            i = end;
            continue;
        }
        let Ok(value) = digits.parse::<f64>() else {
            i = end.max(begin + 1);
            continue;
        };
        let places = decimals.unwrap_or(0);
        out.push(Written {
            text: chars[begin..end].iter().collect(),
            value: value * scale,
            slack: 0.5 * 10f64.powi(-(places as i32)) * scale,
            percent,
            small_count: decimals.is_none() && scale == 1.0 && value <= 10.0,
        });
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(text: &str) -> Vec<(String, f64)> {
        numbers(text)
            .into_iter()
            .map(|w| (w.text, w.value))
            .collect()
    }

    #[test]
    fn reads_numbers_as_written() {
        assert_eq!(
            values("Full DPS 1,234,567 → 1.3M (+12.5%), life 4.2k, PoE2 0.23.1, x2, 3rd"),
            vec![
                ("1,234,567".into(), 1_234_567.0),
                ("1.3M".into(), 1_300_000.0),
                ("12.5%".into(), 12.5),
                ("4.2k".into(), 4_200.0),
            ]
        );
        assert_eq!(values("75%."), vec![("75%".into(), 75.0)]);
        assert_eq!(values("1,23"), vec![("1".into(), 1.0), ("23".into(), 23.0)]);
    }

    #[test]
    fn flags_only_numbers_nobody_gave() {
        let mut evidence = Evidence::default();
        evidence.add_text(r#"{"FullDPS": 1234567.89, "FireResist": 68, "MissingFireResist": 7, "HitMulti": 0.28}"#);
        evidence.add_text("change 12.47 percent");
        let answer = "Full DPS is 1.23M; fire res is 68% (28% taken), +12.5% damage. \
                      You need 7% more, so take 2 rings. Chaos res would reach 31%.";
        assert_eq!(
            evidence.check(answer),
            Check {
                numbers: 6,
                unverified: vec!["31%".to_owned()]
            }
        );
    }

    #[test]
    fn precision_follows_the_digits_written() {
        let mut evidence = Evidence::default();
        evidence.add_text("4180");
        assert!(evidence.unverified("about 4.2k life").is_empty());
        assert_eq!(evidence.unverified("4,250 life"), vec!["4,250".to_owned()]);
    }
}
