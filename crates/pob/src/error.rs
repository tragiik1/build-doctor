use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("PoB2 {0} is not installed")]
    NotInstalled(String),

    #[error("not a PoB2 install: {0}")]
    NotAnInstall(String),

    #[error("PoB2 install is corrupt: {0}")]
    Corrupt(String),

    #[error("download failed: {0}")]
    Download(String),

    #[error("invalid build code: {0}")]
    Code(String),

    #[error("invalid build XML: {0}")]
    Xml(String),

    #[error("PoB failed to start: {0}")]
    Startup(String),

    #[error("PoB reported an error: {0}")]
    Pob(String),

    /// Something the user can act on, already worded for them (an item
    /// text PoB cannot read, a slot an item does not fit).
    #[error("{0}")]
    Message(String),

    /// PoB's output kept changing between recalculations, so no single
    /// answer exists. Seen with PoB bugs where a calculation feeds back into
    /// the build (for example an item line rewritten on every pass).
    #[error(
        "Path of Building's numbers for this build change every time it recalculates ({passes} times in a row), which is a Path of Building bug, so no number has a single answer and none is shown.{}",
        what_changes(.changing)
    )]
    Unsettled { passes: u32, changing: Vec<String> },

    /// Kept as text so `Error` is `Send + Sync` (mlua's error is not).
    #[error("Lua error: {0}")]
    Lua(String),
}

impl From<mlua::Error> for Error {
    fn from(err: mlua::Error) -> Self {
        Self::Lua(err.to_string())
    }
}

impl Error {
    pub(crate) fn io(
        action: &'static str,
        path: impl Into<PathBuf>,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            action,
            path: path.into(),
            source,
        }
    }
}

/// " Still changing: Spirit, SpiritUnreserved, +11794 to Spirit." from the
/// changed lines of PoB's saved output ("<PlayerStat stat=\"Spirit\" …/> ->
/// …", "{enchant}{rune}+11794 to Spirit -> …").
fn what_changes(changing: &[String]) -> String {
    let mut names: Vec<String> = Vec::new();
    for line in changing {
        let before = line.split(" -> ").next().unwrap_or(line);
        let name = match before.split_once("stat=\"") {
            Some((_, rest)) => rest.split('"').next().unwrap_or(rest).to_owned(),
            None => before
                .rsplit('}')
                .next()
                .unwrap_or(before)
                .trim()
                .to_owned(),
        };
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return String::new();
    }
    let shown = names.len().min(4);
    let more = names.len() - shown;
    format!(
        " Still changing: {}{}.",
        names[..shown].join(", "),
        if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_what_keeps_changing() {
        let err = Error::Unsettled {
            passes: 8,
            changing: vec![
                r#"<PlayerStat value="11882" stat="Spirit"/> -> <PlayerStat value="23440" stat="Spirit"/>"#.into(),
                "{enchant}{rune}+11794 to Spirit -> {enchant}{rune}+23352 to Spirit".into(),
            ],
        };
        let text = err.to_string();
        assert!(text.contains("8 times in a row"), "{text}");
        assert!(
            text.ends_with("Still changing: Spirit, +11794 to Spirit."),
            "{text}"
        );
    }
}
