//! Heuristic entity extraction for memories written without structure, and
//! n-gram generation for alias (gazetteer) lookup. Pure functions.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Surface text as written.
    pub name: String,
    /// "proper" | "quoted" | "handle" | "hashtag" | "url" | "path" | "identifier" | "version"
    pub hint: &'static str,
    /// Byte offsets into the input.
    pub start: usize,
    pub end: usize,
}

/// Candidate entities: capitalised multi-word spans (not just a
/// sentence-initial common word), quoted spans, @handles, #hashtags, URLs,
/// file paths, code identifiers (snake_case, camelCase, dotted.names),
/// versioned names ("Postgres 16"). Deduped by norm_name, document order.
pub fn heuristic_entities(text: &str) -> Vec<Candidate> {
    todo!("lane extract")
}

/// All word n-grams (1..=max_n) of `text` as (norm_name(ngram), start, end)
/// byte offsets, for exact alias lookup. Skips n-grams that are only stopwords.
pub fn ngrams(text: &str, max_n: usize) -> Vec<(String, usize, usize)> {
    todo!("lane extract")
}
