//! Optional, deterministic extraction tier for plain-text memories.
//!
//! This is intentionally a small System-1 pass, not a language model. It
//! handles high-value agent sentence patterns and returns no guess when a
//! pattern is ambiguous. Set `NEOCORTEX_EXTRACTOR=rules` to enable it.
//!
//! shortcut: 16 verb patterns and 2 subject shapes, not a parser; upgrade or
//! replace with the GLiNER2.5 candle port in docs/research/extraction.md.

use crate::engine::{EntityIn, RelationIn};
use crate::extract;
use crate::temporal::{self, Clock};

#[derive(Clone, Debug, Default)]
pub struct Extraction {
    pub entities: Vec<EntityIn>,
    pub relations: Vec<RelationIn>,
    pub valid_from: Option<String>,
}

const PATTERNS: &[(&str, &str)] = &[
    ("no longer works at", "works at"),
    ("does not work at", "works at"),
    ("doesn't work at", "works at"),
    ("is employed at", "works at"),
    ("works for", "works for"),
    ("works at", "works at"),
    ("reports to", "reports to"),
    ("lives in", "lives in"),
    ("resides in", "lives in"),
    ("moved to", "lives in"),
    ("joined", "joined"),
    ("manages", "manages"),
    ("prefers", "prefers"),
    ("likes", "likes"),
    ("uses", "uses"),
    ("owns", "owns"),
];

pub fn extract_text(text: &str, clock: &Clock) -> Extraction {
    let mut out = Extraction {
        valid_from: date_in_text(text, clock),
        ..Extraction::default()
    };
    for candidate in extract::heuristic_entities(text) {
        add_entity(&mut out.entities, &candidate.name, None);
    }

    let lower = text.to_lowercase();
    // Longer patterns come first in PATTERNS; a shorter one inside an accepted
    // match ("works at" inside "no longer works at") must not fire again.
    let mut covered: Vec<(usize, usize)> = Vec::new();
    for &(needle, predicate) in PATTERNS {
        let mut from = 0;
        while let Some(relative) = lower[from..].find(needle) {
            let start = from + relative;
            let before = &text[..start];
            let after_start = start + needle.len();
            let after = &text[after_start..];
            from = after_start;
            if covered.iter().any(|&(s, e)| start < e && after_start > s) {
                continue;
            }
            let source = subject(before);
            let target = object(after);
            if let (Some(source), Some(target)) = (source, target) {
                covered.push((start, after_start));
                let negated = needle.starts_with("no longer")
                    || needle.starts_with("does not")
                    || needle.starts_with("doesn't");
                // No types: a guessed type that disagrees with a stored one
                // would split or reject the entity.
                add_entity(&mut out.entities, &source, None);
                add_entity(&mut out.entities, &target, None);
                out.relations.push(RelationIn {
                    source,
                    predicate: predicate.to_string(),
                    target,
                    fact: None,
                    // Single-valuedness comes from the predicate seeds.
                    exclusive: None,
                    negated: Some(negated),
                    valid_from: None,
                    valid_to: None,
                });
            }
        }
    }
    out.relations.sort_by(|a, b| {
        (&a.source, &a.predicate, &a.target).cmp(&(&b.source, &b.predicate, &b.target))
    });
    out.relations.dedup_by(|a, b| {
        a.source == b.source && a.predicate == b.predicate && a.target == b.target
    });
    out.entities.sort_by_key(|a| a.name.to_lowercase());
    out.entities
        .dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    out
}

fn subject(before: &str) -> Option<String> {
    let clause = before
        .rsplit_once(['!', '?', ';', ','])
        .map_or(before, |(_, tail)| tail)
        .trim();
    let words: Vec<&str> = clause.split_whitespace().collect();
    let end = words
        .iter()
        .rposition(|word| is_subject_boundary(word))
        .unwrap_or(words.len());
    let words = &words[..end];
    let end = end.min(words.len());
    let mut start = end.saturating_sub(1);
    while start > 0
        && (words[start - 1]
            .chars()
            .next()
            .is_some_and(|c| c.is_uppercase())
            || words[start - 1].eq_ignore_ascii_case("of"))
    {
        start -= 1;
    }
    let name = words.get(start..).unwrap_or_default().join(" ");
    clean_name(&name)
}

fn object(after: &str) -> Option<String> {
    let first_clause = after
        .split(['.', '!', '?', ';', ','])
        .next()
        .unwrap_or(after);
    let words: Vec<&str> = first_clause.split_whitespace().collect();
    let words = words
        .iter()
        .take_while(|word| {
            !matches!(
                word.to_ascii_lowercase().as_str(),
                "as" | "who"
                    | "that"
                    | "and"
                    | "but"
                    | "on"
                    | "in"
                    | "at"
                    | "since"
                    | "until"
                    | "from"
                    | "during"
                    | "after"
                    | "before"
            )
        })
        .copied()
        .collect::<Vec<_>>();
    clean_name(&words.join(" "))
}

fn clean_name(name: &str) -> Option<String> {
    let name = name
        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
        .trim();
    if name.is_empty() || name.eq_ignore_ascii_case("the user") || name.len() < 2 {
        return None;
    }
    Some(name.trim_end_matches('.').to_string())
}

fn is_subject_boundary(word: &str) -> bool {
    matches!(
        word.trim_matches(|c: char| !c.is_alphanumeric()),
        "I" | "The" | "the" | "A" | "An" | "an" | "Owner:" | "owner:"
    )
}

fn add_entity(entities: &mut Vec<EntityIn>, name: &str, kind: Option<String>) {
    if entities
        .iter()
        .any(|entity| entity.name.eq_ignore_ascii_case(name))
    {
        return;
    }
    entities.push(EntityIn {
        name: name.to_string(),
        kind,
        description: None,
        aliases: Vec::new(),
        same_as: None,
    });
}

fn date_in_text(text: &str, clock: &Clock) -> Option<String> {
    for token in text.split_whitespace() {
        let token = token.trim_matches(|c: char| !c.is_ascii_digit() && c != '-');
        if token.len() == 10
            && token.as_bytes().get(4) == Some(&b'-')
            && token.as_bytes().get(7) == Some(&b'-')
            && token.chars().all(|c| c.is_ascii_digit() || c == '-')
        {
            return Some(token.to_string());
        }
    }
    temporal::find_window(text, clock).map(|window| window.matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_titles_with_periods_and_retractions() {
        let result = extract_text("Dr. Patel no longer works at Acme.", &Clock::system());
        assert!(result.entities.iter().any(|e| e.name == "Dr. Patel"));
        assert!(result.relations.iter().any(|r| r.negated == Some(true)));
    }
}
