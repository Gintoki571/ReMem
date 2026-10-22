//! Text utilities shared by extraction, resolution and search. Pure functions.

/// Lowercase, strip common Latin accents, map punctuation runs to one space,
/// collapse whitespace, trim. Keeps letters/digits of any script.
pub fn fold(s: &str) -> String {
    todo!("lane text")
}

/// Canonical entity key: fold, drop leading articles/possessive determiners
/// (the, a, an, my, our, your, his, her, their), drop trailing "'s",
/// singularise the last token. "The Houses" -> "house", "my bros" -> "bro".
pub fn norm_name(s: &str) -> String {
    todo!("lane text")
}

/// English singular for one lowercase word; leaves words that only look
/// plural alone (analysis, status, bus, glass, physics, news, series).
pub fn singularize(w: &str) -> String {
    todo!("lane text")
}

/// Jaro-Winkler similarity in [0,1] (prefix scale 0.1, max prefix 4).
pub fn jaro_winkler(a: &str, b: &str) -> f64 {
    todo!("lane text")
}

/// Jaccard similarity of padded character trigram sets, in [0,1].
pub fn trigram_jaccard(a: &str, b: &str) -> f64 {
    todo!("lane text")
}

/// Uppercase initials of a multi-token name ("New York City" -> "NYC");
/// None for single tokens.
pub fn acronym(name: &str) -> Option<String> {
    todo!("lane text")
}

/// English stopword test on a folded token.
pub fn is_stopword(w: &str) -> bool {
    todo!("lane text")
}

/// Folded word tokens (split on non-alphanumerics).
pub fn tokens(s: &str) -> Vec<String> {
    todo!("lane text")
}

/// Safe FTS5 MATCH expression: OR of quoted non-stopword tokens of `q` and
/// `extra`; prefix arm (`"tok"*`) for tokens >= 4 chars. None when nothing
/// searchable remains. Never lets FTS5 syntax through.
pub fn fts_query(q: &str, extra: &[String]) -> Option<String> {
    todo!("lane text")
}

/// Canonical predicate key: fold, drop auxiliaries (is, are, was, were, be,
/// been, being, has, have, had, currently, now), lemmatise each verb-ish word
/// by suffix rules (works/working/worked -> work, lives/lived -> live),
/// join with one space. "Lives_In" -> "live in", "is working at" -> "work at".
pub fn norm_predicate(p: &str) -> String {
    todo!("lane text")
}

/// Dedup key for a memory: FNV-1a 64 hex of kind + "\0" + fold(content).
pub fn content_hash(kind: &str, content: &str) -> String {
    todo!("lane text")
}
