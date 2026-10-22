//! Text utilities shared by extraction, resolution and search. Pure functions.

/// Lowercase, strip common Latin accents, map punctuation runs to one space,
/// collapse whitespace, trim. Keeps letters/digits of any script.
pub fn fold(s: &str) -> String {
    let chars: Vec<char> = s.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for (i, &c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            push_ascii(&mut out, c);
        } else if !is_inner_apostrophe(&chars, i) {
            pending_space = true;
        }
    }
    out
}

fn is_inner_apostrophe(chars: &[char], i: usize) -> bool {
    i > 0
        && i + 1 < chars.len()
        && matches!(chars[i], '\'' | '’' | 'ʼ')
        && chars[i - 1].is_alphanumeric()
        && chars[i + 1].is_alphanumeric()
}

fn push_ascii(out: &mut String, c: char) {
    let mapped = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'ç' | 'ć' | 'č' => "c",
        'ď' | 'đ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
        'ğ' => "g",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' => "i",
        'ł' | 'ĺ' | 'ľ' => "l",
        'ñ' | 'ń' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
        'ŕ' | 'ř' => "r",
        'ś' | 'š' | 'ş' => "s",
        'ť' | 'ţ' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => "u",
        'ý' | 'ÿ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        'þ' => "th",
        'ð' => "d",
        _ => {
            out.push(c);
            return;
        }
    };
    out.push_str(mapped);
}

/// Canonical entity key: fold, drop leading articles/possessive determiners
/// (the, a, an, my, our, your, his, her, their), drop trailing "'s",
/// singularise the last token. "The Houses" -> "house", "my bros" -> "bro".
pub fn norm_name(s: &str) -> String {
    const DETERMINERS: [&str; 9] = ["the", "a", "an", "my", "our", "your", "his", "her", "their"];
    let folded = fold(strip_possessive(s));
    let mut words: Vec<String> = folded.split_whitespace().map(str::to_string).collect();
    while words
        .first()
        .is_some_and(|w| DETERMINERS.contains(&w.as_str()))
    {
        words.remove(0);
    }
    if words.is_empty() {
        return folded;
    }
    if let Some(last) = words.last_mut() {
        *last = singularize(last);
    }
    words.join(" ")
}

fn strip_possessive(s: &str) -> &str {
    let trimmed = s.trim_end();
    for suffix in ["'s", "’s", "'S", "’S"] {
        if let Some(stem) = trimmed.strip_suffix(suffix) {
            return stem;
        }
    }
    trimmed
}

/// English singular for one lowercase word; leaves words that only look
/// plural alone (analysis, status, bus, glass, physics, news, series).
pub fn singularize(w: &str) -> String {
    const IRREGULAR: [(&str, &str); 8] = [
        ("children", "child"),
        ("people", "person"),
        ("men", "man"),
        ("women", "woman"),
        ("mice", "mouse"),
        ("geese", "goose"),
        ("teeth", "tooth"),
        ("feet", "foot"),
    ];
    const INVARIANT: [&str; 30] = [
        "analysis",
        "basis",
        "crisis",
        "status",
        "bus",
        "gas",
        "lens",
        "news",
        "series",
        "species",
        "physics",
        "mathematics",
        "chaos",
        "this",
        "is",
        "was",
        "has",
        "its",
        "us",
        "yes",
        "plus",
        "kubernetes",
        "redis",
        "postgres",
        "windows",
        "macos",
        "ios",
        "aws",
        "js",
        "css",
    ];
    for (plural, singular) in IRREGULAR {
        if w == plural {
            return singular.to_string();
        }
    }
    if INVARIANT.contains(&w) || w.ends_with("ss") || w.ends_with("us") || w.ends_with("is") {
        return w.to_string();
    }
    if w.chars().count() <= 3 {
        return w.to_string();
    }
    if let Some(stem) = w.strip_suffix("ies") {
        if stem.chars().count() >= 2 {
            return format!("{stem}y");
        }
    }
    if let Some(stem) = w.strip_suffix("ives") {
        return format!("{stem}ife");
    }
    if let Some(stem) = w.strip_suffix("lves") {
        return format!("{stem}lf");
    }
    if let Some(stem) = w.strip_suffix("es") {
        // buses/viruses -> bus/virus, but houses/causes -> house/cause.
        let c: Vec<char> = stem.chars().collect();
        if stem.ends_with("us") && c.len() >= 3 && !is_vowel(c[c.len() - 3]) {
            return stem.to_string();
        }
        // heroes/potatoes -> hero/potato, but shoes/toes keep their e.
        if stem.ends_with('o') && !["sho", "to", "cano", "obo", "do", "go", "flo"].contains(&stem) {
            return stem.to_string();
        }
        if ["ss", "sh", "ch", "x", "z"]
            .iter()
            .any(|ending| stem.ends_with(ending))
        {
            return stem.to_string();
        }
    }
    if let Some(stem) = w.strip_suffix('s') {
        return stem.to_string();
    }
    w.to_string()
}

/// Jaro-Winkler similarity in [0,1] (prefix scale 0.1, max prefix 4).
pub fn jaro_winkler(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let jaro = jaro(&a, &b);
    let prefix = a
        .iter()
        .zip(b.iter())
        .take(4)
        .take_while(|(x, y)| x == y)
        .count();
    (jaro + prefix as f64 * 0.1 * (1.0 - jaro)).min(1.0)
}

fn jaro(a: &[char], b: &[char]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let window = (a.len().max(b.len()) / 2).saturating_sub(1);
    let mut a_hit = vec![false; a.len()];
    let mut b_hit = vec![false; b.len()];
    let mut matches = 0usize;
    for (i, ca) in a.iter().enumerate() {
        let lo = i.saturating_sub(window);
        let hi = (i + window + 1).min(b.len());
        for (j, cb) in b.iter().enumerate().take(hi).skip(lo) {
            if !b_hit[j] && cb == ca {
                a_hit[i] = true;
                b_hit[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    let mut transpositions = 0usize;
    let mut j = 0usize;
    for (i, ca) in a.iter().enumerate() {
        if a_hit[i] {
            while !b_hit[j] {
                j += 1;
            }
            if ca != &b[j] {
                transpositions += 1;
            }
            j += 1;
        }
    }
    let m = matches as f64;
    let t = transpositions as f64 / 2.0;
    (m / a.len() as f64 + m / b.len() as f64 + (m - t) / m) / 3.0
}

/// Jaccard similarity of padded character trigram sets, in [0,1].
pub fn trigram_jaccard(a: &str, b: &str) -> f64 {
    fn grams(s: &str) -> std::collections::HashSet<(char, char, char)> {
        let padded: Vec<char> = format!("  {s}  ").chars().collect();
        padded.windows(3).map(|w| (w[0], w[1], w[2])).collect()
    }
    let a = grams(a);
    let b = grams(b);
    let union = a.union(&b).count();
    if union == 0 {
        return 1.0;
    }
    a.intersection(&b).count() as f64 / union as f64
}

/// Uppercase initials of a multi-token name ("New York City" -> "NYC");
/// None for single tokens.
pub fn acronym(name: &str) -> Option<String> {
    let words: Vec<&str> = name.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let mut out = String::new();
    for word in words {
        if word.eq_ignore_ascii_case("of")
            || word.eq_ignore_ascii_case("and")
            || word.eq_ignore_ascii_case("the")
        {
            continue;
        }
        if let Some(c) = word.chars().find(|c| c.is_alphanumeric()) {
            out.extend(c.to_uppercase());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// English stopword test on a folded token.
pub fn is_stopword(w: &str) -> bool {
    const STOPWORDS: &[&str] = &[
        "a",
        "about",
        "above",
        "across",
        "actually",
        "after",
        "again",
        "against",
        "all",
        "almost",
        "also",
        "although",
        "always",
        "am",
        "among",
        "an",
        "and",
        "another",
        "any",
        "anyone",
        "anything",
        "are",
        "around",
        "as",
        "at",
        "be",
        "became",
        "because",
        "become",
        "been",
        "before",
        "being",
        "below",
        "between",
        "both",
        "but",
        "by",
        "can",
        "cannot",
        "could",
        "did",
        "do",
        "does",
        "doing",
        "done",
        "down",
        "during",
        "each",
        "either",
        "else",
        "even",
        "ever",
        "every",
        "few",
        "for",
        "from",
        "further",
        "get",
        "gets",
        "getting",
        "got",
        "had",
        "has",
        "have",
        "having",
        "he",
        "her",
        "here",
        "hers",
        "herself",
        "him",
        "himself",
        "his",
        "how",
        "however",
        "i",
        "if",
        "in",
        "into",
        "is",
        "it",
        "its",
        "itself",
        "just",
        "let",
        "like",
        "make",
        "many",
        "may",
        "me",
        "might",
        "more",
        "most",
        "much",
        "must",
        "my",
        "myself",
        "never",
        "no",
        "nor",
        "not",
        "now",
        "of",
        "off",
        "on",
        "once",
        "only",
        "or",
        "other",
        "others",
        "our",
        "ours",
        "ourselves",
        "out",
        "over",
        "own",
        "per",
        "please",
        "quite",
        "rather",
        "really",
        "same",
        "she",
        "should",
        "since",
        "so",
        "some",
        "someone",
        "something",
        "still",
        "such",
        "than",
        "that",
        "the",
        "their",
        "theirs",
        "them",
        "themselves",
        "then",
        "there",
        "these",
        "they",
        "this",
        "those",
        "though",
        "through",
        "thus",
        "to",
        "today",
        "together",
        "tomorrow",
        "too",
        "under",
        "until",
        "up",
        "upon",
        "us",
        "very",
        "via",
        "was",
        "we",
        "were",
        "what",
        "when",
        "where",
        "whether",
        "which",
        "while",
        "who",
        "whom",
        "why",
        "will",
        "with",
        "would",
        "yet",
        "yesterday",
        "you",
        "your",
        "yours",
        "yourself",
        "yourselves",
    ];
    STOPWORDS.contains(&w)
}

/// Folded word tokens (split on non-alphanumerics).
pub fn tokens(s: &str) -> Vec<String> {
    fold(s).split_whitespace().map(str::to_string).collect()
}

/// Safe FTS5 MATCH expression: OR of quoted non-stopword tokens of `q` and
/// `extra`; prefix arm (`"tok"*`) for tokens >= 4 chars. None when nothing
/// searchable remains. Never lets FTS5 syntax through.
pub fn fts_query(q: &str, extra: &[String]) -> Option<String> {
    let mut kept: Vec<String> = Vec::new();
    let mut push = |token: String| {
        if token.chars().count() < 2 || is_stopword(&token) || kept.contains(&token) {
            return;
        }
        kept.push(token);
    };
    for token in tokens(q) {
        push(token);
    }
    for text in extra {
        for token in tokens(text) {
            push(token);
        }
    }
    if kept.is_empty() {
        return None;
    }
    let arms: Vec<String> = kept
        .iter()
        .map(|token| {
            if token.chars().count() >= 4 {
                format!("\"{token}\"*")
            } else {
                format!("\"{token}\"")
            }
        })
        .collect();
    Some(arms.join(" OR "))
}

/// Canonical predicate key: fold, drop auxiliaries (is, are, was, were, be,
/// been, being, has, have, had, currently, now), lemmatise each verb-ish word
/// by suffix rules (works/working/worked -> work, lives/lived -> live),
/// join with one space. "Lives_In" -> "live in", "is working at" -> "work at".
pub fn norm_predicate(p: &str) -> String {
    const AUXILIARIES: [&str; 12] = [
        "is",
        "are",
        "was",
        "were",
        "be",
        "been",
        "being",
        "has",
        "have",
        "had",
        "currently",
        "now",
    ];
    let words: Vec<String> = tokens(p)
        .into_iter()
        .filter(|w| !matches!(w.as_str(), "a" | "an" | "the"))
        .collect();
    let content: Vec<&String> = words
        .iter()
        .filter(|w| !AUXILIARIES.contains(&w.as_str()))
        .collect();
    // "has a" / "is" carry meaning only through the auxiliary itself.
    let kept = if content.is_empty() {
        words.iter().collect()
    } else {
        content
    };
    kept.iter()
        .map(|w| lemma_verb(w))
        .collect::<Vec<_>>()
        .join(" ")
}

fn lemma_verb(w: &str) -> String {
    // "married" stays an adjective: "married to", not "marry to".
    if w == "married" {
        return w.to_string();
    }
    const IRREGULAR: [(&str, &str); 24] = [
        ("is", "be"),
        ("are", "be"),
        ("was", "be"),
        ("were", "be"),
        ("been", "be"),
        ("being", "be"),
        ("has", "have"),
        ("had", "have"),
        ("does", "do"),
        ("did", "do"),
        ("done", "do"),
        ("went", "go"),
        ("gone", "go"),
        ("made", "make"),
        ("got", "get"),
        ("took", "take"),
        ("knew", "know"),
        ("met", "meet"),
        ("bought", "buy"),
        ("left", "leave"),
        ("built", "build"),
        ("ran", "run"),
        ("wrote", "write"),
        ("taught", "teach"),
    ];
    if let Some((_, base)) = IRREGULAR.iter().find(|(f, _)| *f == w) {
        return base.to_string();
    }
    if w.chars().count() <= 3 {
        return w.to_string();
    }
    if let Some(stem) = w.strip_suffix("ied") {
        return format!("{stem}y");
    }
    if let Some(stem) = w.strip_suffix("ing") {
        return restore_stem(stem);
    }
    if let Some(stem) = w.strip_suffix("ed") {
        return restore_stem(stem);
    }
    if let Some(stem) = w.strip_suffix("ies") {
        if stem.chars().count() >= 2 {
            return format!("{stem}y");
        }
    }
    if let Some(stem) = w.strip_suffix("es") {
        if ["ss", "sh", "ch", "x", "z"]
            .iter()
            .any(|ending| stem.ends_with(ending))
        {
            return stem.to_string();
        }
    }
    if let Some(stem) = w.strip_suffix('s') {
        return stem.to_string();
    }
    w.to_string()
}

fn restore_stem(stem: &str) -> String {
    let chars: Vec<char> = stem.chars().collect();
    let n = chars.len();
    if n >= 3 {
        if chars[n - 1] == chars[n - 2] && !matches!(chars[n - 1], 'l' | 's' | 'z') {
            return chars[..n - 1].iter().collect();
        }
        let (c1, c2, c3) = (chars[n - 3], chars[n - 2], chars[n - 1]);
        if !is_vowel(c1) && is_vowel(c2) && !is_vowel(c3) && !matches!(c3, 'w' | 'x' | 'y') {
            return format!("{stem}e");
        }
    } else if n == 2 {
        return format!("{stem}e");
    }
    stem.to_string()
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Dedup key for a memory: FNV-1a 64 hex of kind + "\0" + fold(content).
pub fn content_hash(kind: &str, content: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in kind
        .bytes()
        .chain(std::iter::once(0))
        .chain(fold(content).bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_lowercases_strips_accents_and_punctuation() {
        assert_eq!(fold("Café ÜBER Niño"), "cafe uber nino");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("naïve façade"), "naive facade");
        assert_eq!(fold("O'Brien"), "obrien");
        assert_eq!(fold("User’s"), "users");
        assert_eq!(fold("  Hello,   WORLD!! "), "hello world");
        assert_eq!(fold("東京 Rust 2026"), "東京 rust 2026");
        assert_eq!(fold("a—b/c"), "a b c");
        assert_eq!(fold(""), "");
    }

    #[test]
    fn norm_name_examples() {
        for (input, want) in [
            ("The Houses", "house"),
            ("my bros", "bro"),
            ("Mom's", "mom"),
            ("Mom’s", "mom"),
            ("PostgreSQL 16", "postgresql 16"),
            ("New York City", "new york city"),
            ("the user", "user"),
            ("Analysis", "analysis"),
            ("Status", "status"),
            ("Series", "series"),
            ("children", "child"),
            ("people", "person"),
            ("mice", "mouse"),
            ("boxes", "box"),
            ("cities", "city"),
            ("knives", "knife"),
            ("class", "class"),
            ("glasses", "glass"),
        ] {
            assert_eq!(norm_name(input), want, "input {input:?}");
        }
        // stripping articles must never empty a non-empty name
        assert_eq!(norm_name("the"), "the");
    }

    #[test]
    fn singularize_examples() {
        for (input, want) in [
            ("bros", "bro"),
            ("friends", "friend"),
            ("houses", "house"),
            ("cars", "car"),
            ("children", "child"),
            ("people", "person"),
            ("men", "man"),
            ("women", "woman"),
            ("mice", "mouse"),
            ("geese", "goose"),
            ("teeth", "tooth"),
            ("feet", "foot"),
            ("cities", "city"),
            ("boxes", "box"),
            ("glasses", "glass"),
            ("knives", "knife"),
            ("analysis", "analysis"),
            ("basis", "basis"),
            ("crisis", "crisis"),
            ("status", "status"),
            ("bus", "bus"),
            ("gas", "gas"),
            ("lens", "lens"),
            ("news", "news"),
            ("series", "series"),
            ("species", "species"),
            ("physics", "physics"),
            ("mathematics", "mathematics"),
            ("chaos", "chaos"),
            ("kubernetes", "kubernetes"),
            ("redis", "redis"),
            ("windows", "windows"),
            ("aws", "aws"),
            ("css", "css"),
        ] {
            assert_eq!(singularize(input), want, "input {input:?}");
        }
    }

    #[test]
    fn jaro_winkler_reference_values() {
        assert!((jaro_winkler("martha", "marhta") - 0.9611).abs() < 0.001);
        assert!((jaro_winkler("dixon", "dicksonx") - 0.8133).abs() < 0.001);
        assert_eq!(jaro_winkler("same", "same"), 1.0);
        assert_eq!(jaro_winkler("", ""), 1.0);
        assert_eq!(jaro_winkler("abc", ""), 0.0);
        assert_eq!(jaro_winkler("", "abc"), 0.0);
        assert_eq!(jaro_winkler("abc", "xyz"), 0.0);
    }

    #[test]
    fn trigram_jaccard_pads_and_compares() {
        assert_eq!(trigram_jaccard("hello", "hello"), 1.0);
        assert_eq!(trigram_jaccard("abc", "xyz"), 0.0);
        let v = trigram_jaccard("night", "nacht");
        assert!(v > 0.0 && v < 1.0, "night/nacht = {v}");
        assert_eq!(trigram_jaccard("", ""), 1.0);
    }

    #[test]
    fn acronym_uses_initials_and_skips_connectors() {
        assert_eq!(acronym("New York City").as_deref(), Some("NYC"));
        assert_eq!(acronym("United States of America").as_deref(), Some("USA"));
        assert_eq!(acronym("Google"), None);
        assert_eq!(acronym("the").as_deref(), None);
    }

    #[test]
    fn stopwords_cover_questions_auxiliaries_pronouns() {
        for w in [
            "what", "which", "who", "when", "where", "how", "why", "is", "are", "was", "were",
            "my", "your", "the", "and", "not", "they", "we", "i",
        ] {
            assert!(is_stopword(w), "{w} should be a stopword");
        }
        for w in ["postgres", "audit", "alice", "rust", "production"] {
            assert!(!is_stopword(w), "{w} should not be a stopword");
        }
    }

    #[test]
    fn tokens_fold_then_split() {
        assert_eq!(
            tokens("O'Brien, PostgreSQL-16!"),
            vec!["obrien", "postgresql", "16"]
        );
        assert!(tokens("   ").is_empty());
    }

    #[test]
    fn fts_query_is_safe_and_prefixes_long_tokens() {
        assert_eq!(
            fts_query("what is the production database?", &[]).as_deref(),
            Some("\"production\"* OR \"database\"*")
        );
        assert_eq!(
            fts_query("auditors", &["audit logs".to_string()]).as_deref(),
            Some("\"auditors\"* OR \"audit\"* OR \"logs\"*")
        );
        assert_eq!(fts_query("what is the", &[]), None);
        assert_eq!(fts_query("x y a", &[]), None);
        // FTS5 syntax in the input must be tokenised away, never passed through.
        assert_eq!(
            fts_query("NEAR(\"foo\" bar) OR *", &[]).as_deref(),
            Some("\"near\"* OR \"foo\" OR \"bar\"")
        );
    }

    #[test]
    fn fts_query_matches_real_index() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let c = db.sql();
        for (i, content) in [
            "Auditors review the production db every quarter",
            "The deployment ran at noon",
            "Production database backups run nightly in postgres",
            "We planted tomatoes in the garden",
        ]
        .iter()
        .enumerate()
        {
            c.execute(
                "INSERT INTO memories(content, kind, created_at, valid_from, content_hash) \
                 VALUES (?1, 'fact', 0, 0, ?2)",
                rusqlite::params![content, format!("h{i}")],
            )
            .unwrap();
        }
        let hits = |q: &str| -> i64 {
            let expr = fts_query(q, &[]).expect("query");
            c.query_row(
                "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH ?1",
                [expr],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(hits("auditors") >= 1);
        assert!(hits("running deployments") >= 1);
        assert!(hits("what is the PRODUCTION database?") >= 2);
        assert_eq!(fts_query("what is the", &[]), None);
    }

    #[test]
    fn norm_predicate_lemmatises_verbs() {
        for (input, want) in [
            ("Lives_In", "live in"),
            ("lives in", "live in"),
            ("lived in", "live in"),
            ("is working at", "work at"),
            ("works at", "work at"),
            ("worked at", "work at"),
            ("has been using", "use"),
            ("uses", "use"),
            ("prefers", "prefer"),
            ("is married to", "married to"),
        ] {
            assert_eq!(norm_predicate(input), want, "input {input:?}");
        }
    }

    #[test]
    fn content_hash_is_stable_fold_based_fnv() {
        let h = content_hash("fact", "Hello  WORLD!");
        assert_eq!(h.len(), 16);
        assert!(h
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
        assert_eq!(h, content_hash("fact", "hello world"));
        assert_ne!(h, content_hash("event", "hello world"));
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;

    #[test]
    fn singular_and_predicate_edge_cases() {
        for (p, s) in [
            ("buses", "bus"),
            ("viruses", "virus"),
            ("houses", "house"),
            ("causes", "cause"),
            ("heroes", "hero"),
            ("shoes", "shoe"),
        ] {
            assert_eq!(singularize(p), s, "{p}");
        }
        for (p, k) in [
            ("studied", "study"),
            ("has a", "have"),
            ("is the manager of", "manager of"),
            ("is", "be"),
            ("met", "meet"),
            ("lives in", "live in"),
        ] {
            assert_eq!(norm_predicate(p), k, "{p}");
        }
    }
}
