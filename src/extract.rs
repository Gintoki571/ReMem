//! Heuristic entity extraction for memories written without structure, and
//! n-gram generation for alias (gazetteer) lookup. Pure functions.

use crate::text::{fold, is_stopword, norm_name, tokens};

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
    let (mut cands, consumed) = scan_specials(text);
    cands.extend(word_candidates(text, &consumed));
    cands.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| hint_rank(a.hint).cmp(&hint_rank(b.hint)))
    });
    let mut seen: Vec<String> = Vec::new();
    cands.retain(|c| {
        let norm = norm_name(&c.name);
        if norm.is_empty() || seen.contains(&norm) {
            return false;
        }
        seen.push(norm);
        true
    });
    cands
}

fn hint_rank(hint: &str) -> u8 {
    match hint {
        "url" => 0,
        "quoted" => 1,
        "handle" => 2,
        "hashtag" => 3,
        "path" => 4,
        "identifier" => 5,
        "version" => 6,
        _ => 7,
    }
}

/// URLs, paths, @handles, #hashtags and quoted spans found by raw text scan.
/// Returns the candidates plus the ranges whose text must not also become a
/// word-level candidate (a URL's "example.com" is not a dotted identifier).
fn scan_specials(text: &str) -> (Vec<Candidate>, Vec<(usize, usize)>) {
    let mut cands = Vec::new();
    let mut consumed = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let c = rest.chars().next().expect("char boundary");
        let prev = text[..i].chars().next_back();
        let at_boundary = prev.is_none_or(|p| !p.is_alphanumeric());

        if starts_ignore_case(rest, "https://") || starts_ignore_case(rest, "http://") {
            let end = url_end(text, i);
            cands.push(Candidate {
                name: text[i..end].to_string(),
                hint: "url",
                start: i,
                end,
            });
            consumed.push((i, end));
            i = end;
            continue;
        }

        if c == '@' && at_boundary {
            let mut end = i + 1;
            while let Some(ch) = text[end..].chars().next() {
                if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
                    end += ch.len_utf8();
                } else {
                    break;
                }
            }
            while end > i + 1 {
                let last = text[i + 1..end].chars().next_back().expect("non-empty");
                if matches!(last, '.' | '-') {
                    end -= last.len_utf8();
                } else {
                    break;
                }
            }
            let tag = &text[i + 1..end];
            if tag.chars().any(|ch| ch.is_ascii_alphanumeric()) {
                cands.push(Candidate {
                    name: text[i..end].to_string(),
                    hint: "handle",
                    start: i,
                    end,
                });
                consumed.push((i, end));
            }
            i = end.max(i + 1);
            continue;
        }

        if c == '#' && at_boundary {
            let mut end = i + 1;
            while let Some(ch) = text[end..].chars().next() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    end += ch.len_utf8();
                } else {
                    break;
                }
            }
            let tag = &text[i + 1..end];
            if tag
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphanumeric())
            {
                cands.push(Candidate {
                    name: tag.to_string(),
                    hint: "hashtag",
                    start: i + 1,
                    end,
                });
                consumed.push((i, end));
            }
            i = end.max(i + 1);
            continue;
        }

        if at_boundary {
            if let Some(end) = path_end(text, i) {
                cands.push(Candidate {
                    name: text[i..end].to_string(),
                    hint: "path",
                    start: i,
                    end,
                });
                consumed.push((i, end));
                i = end;
                continue;
            }
        }

        if matches!(c, '"' | '\'' | '`') && at_boundary {
            if let Some((name_start, name_end)) = quoted_span(text, i, c) {
                cands.push(Candidate {
                    name: text[name_start..name_end].to_string(),
                    hint: "quoted",
                    start: name_start,
                    end: name_end,
                });
            }
        }
        i += c.len_utf8();
    }
    (cands, consumed)
}

fn starts_ignore_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn url_end(text: &str, start: usize) -> usize {
    let mut end = start;
    for (off, ch) in text[start..].char_indices() {
        if ch.is_whitespace() {
            break;
        }
        end = start + off + ch.len_utf8();
    }
    trim_trailing_punct(text, start, end)
}

fn trim_trailing_punct(text: &str, start: usize, mut end: usize) -> usize {
    while end > start {
        let last = text[start..end].chars().next_back().expect("non-empty");
        if matches!(
            last,
            '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"' | '`'
        ) {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    end
}

/// End offset of a path starting at `start` (`/a/b`, `./x`, `~/x`, `C:\x`).
fn path_end(text: &str, start: usize) -> Option<usize> {
    let rest = &text[start..];
    let prefix = if rest.starts_with("../") {
        3
    } else if rest.starts_with("./") || rest.starts_with("~/") {
        2
    } else if rest.starts_with('/') {
        1
    } else {
        let bytes = rest.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/')
        {
            3
        } else {
            return None;
        }
    };
    if !rest[prefix..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '~'))
    {
        return None;
    }
    let mut end = start;
    for (off, ch) in rest.char_indices() {
        if ch.is_alphanumeric()
            || matches!(
                ch,
                '/' | '\\' | '.' | '-' | '_' | '~' | '@' | '+' | '%' | ':'
            )
        {
            end = start + off + ch.len_utf8();
        } else {
            break;
        }
    }
    let end = trim_trailing_punct(text, start, end);
    (end > start + prefix).then_some(end)
}

fn quoted_span(text: &str, open: usize, quote: char) -> Option<(usize, usize)> {
    let inner_start = open + quote.len_utf8();
    let mut search = inner_start;
    while let Some(rel) = text[search..].find(quote) {
        let close = search + rel;
        let after = text[close + quote.len_utf8()..].chars().next();
        let inner = &text[inner_start..close];
        let words = inner.split_whitespace().count();
        if after.is_none_or(|c| !c.is_alphanumeric())
            && !inner.contains('\n')
            && (1..=6).contains(&words)
        {
            let trimmed = inner.trim();
            if !trimmed.is_empty() {
                let lead = inner.len() - inner.trim_start().len();
                return Some((inner_start + lead, inner_start + lead + trimmed.len()));
            }
        }
        search = close + quote.len_utf8();
    }
    None
}

struct Word<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

fn is_consumed(ranges: &[(usize, usize)], pos: usize) -> bool {
    ranges.iter().any(|&(start, end)| pos >= start && pos < end)
}

fn word_tokens<'a>(text: &'a str, consumed: &[(usize, usize)]) -> Vec<Word<'a>> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut words = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        if is_consumed(consumed, start) {
            i += 1;
            continue;
        }
        if c == '&' {
            words.push(Word {
                text: &text[start..start + 1],
                start,
                end: start + 1,
            });
            i += 1;
            continue;
        }
        if !c.is_alphanumeric() {
            i += 1;
            continue;
        }
        let mut end = start + c.len_utf8();
        let mut j = i + 1;
        while j < chars.len() {
            let (pos, ch) = chars[j];
            if is_consumed(consumed, pos) {
                break;
            }
            if ch.is_alphanumeric() || is_word_joiner(&chars, j) {
                end = pos + ch.len_utf8();
                j += 1;
            } else {
                break;
            }
        }
        words.push(Word {
            text: &text[start..end],
            start,
            end,
        });
        i = j;
    }
    words
}

fn is_word_joiner(chars: &[(usize, char)], j: usize) -> bool {
    let ch = chars[j].1;
    let prev = j.checked_sub(1).and_then(|k| chars.get(k)).map(|&(_, c)| c);
    let next = chars.get(j + 1).map(|&(_, c)| c);
    match ch {
        '.' | '\'' | '’' | '_' => {
            prev.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric)
        }
        ':' => {
            prev.is_some_and(|p| p.is_alphanumeric() || p == ':')
                && next.is_some_and(|n| n.is_alphanumeric() || n == ':')
        }
        _ => false,
    }
}

fn word_candidates(text: &str, consumed: &[(usize, usize)]) -> Vec<Candidate> {
    let words = word_tokens(text, consumed);
    let mut cands = Vec::new();
    for word in &words {
        if let Some(hint) = word_hint(word.text) {
            cands.push(Candidate {
                name: word.text.to_string(),
                hint,
                start: word.start,
                end: word.end,
            });
        }
    }

    let mut i = 0;
    while i < words.len() {
        if !is_span_start(words[i].text) {
            i += 1;
            continue;
        }
        let mut last = i;
        let mut k = i + 1;
        while k < words.len() {
            if !gap_is_space(text, &words[k - 1], &words[k]) {
                break;
            }
            let w = words[k].text;
            if is_proper_word(w) {
                last = k;
                k += 1;
            } else if is_connector(w)
                && k + 1 < words.len()
                && is_proper_word(words[k + 1].text)
                && gap_is_space(text, &words[k], &words[k + 1])
            {
                k += 1;
            } else if is_number(w) && (k + 1 >= words.len() || !is_proper_word(words[k + 1].text)) {
                last = k;
                break;
            } else {
                break;
            }
        }
        let start = words[i].start;
        if last == i {
            cands.push(Candidate {
                name: words[i].text.to_string(),
                hint: "proper",
                start,
                end: words[i].end,
            });
        } else {
            let end = words[last].end;
            let has_calendar = words[i..=last]
                .iter()
                .any(|w| is_weekday_or_month(&fold(w.text)));
            if !has_calendar {
                let hint = if is_number(words[last].text) {
                    "version"
                } else {
                    "proper"
                };
                cands.push(Candidate {
                    name: text[start..end].to_string(),
                    hint,
                    start,
                    end,
                });
            }
        }
        i = last + 1;
    }
    cands
}

fn gap_is_space(text: &str, left: &Word<'_>, right: &Word<'_>) -> bool {
    text[left.end..right.start].chars().all(char::is_whitespace)
}

fn word_hint(text: &str) -> Option<&'static str> {
    const EXTENSIONS: [&str; 43] = [
        "rs", "py", "js", "ts", "tsx", "jsx", "md", "json", "toml", "yaml", "yml", "txt", "sh",
        "bash", "zsh", "c", "h", "cc", "cpp", "hpp", "go", "java", "rb", "php", "sql", "db",
        "sqlite", "csv", "tsv", "html", "css", "scss", "conf", "cfg", "ini", "log", "lock", "png",
        "jpg", "jpeg", "gif", "svg", "pdf",
    ];
    if let Some((stem, ext)) = text.rsplit_once('.') {
        if !stem.is_empty() && EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
            return Some("path");
        }
    }
    if !text.chars().any(char::is_alphabetic) {
        return None;
    }
    if text.contains('_') || text.contains("::") || text.matches('.').count() >= 2 {
        return Some("identifier");
    }
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if c.is_uppercase() && prev.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit()) {
            return Some("identifier");
        }
        prev = Some(c);
    }
    None
}

fn is_proper_word(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

fn is_number(word: &str) -> bool {
    word.chars().any(|c| c.is_ascii_digit()) && word.chars().all(|c| c.is_ascii_digit() || c == '.')
}

fn is_connector(word: &str) -> bool {
    ["of", "de", "van", "&"]
        .iter()
        .any(|c| word.eq_ignore_ascii_case(c))
}

fn is_span_start(word: &str) -> bool {
    is_proper_word(word) && !excluded_word(word)
}

fn excluded_word(word: &str) -> bool {
    const OPENERS: &[&str] = &[
        "yesterday",
        "today",
        "tomorrow",
        "also",
        "then",
        "however",
        "note",
        "please",
        "finally",
        "first",
        "second",
        "third",
        "next",
        "meanwhile",
        "additionally",
        "furthermore",
        "moreover",
        "still",
        "yet",
        "overall",
        "generally",
        "usually",
        "often",
        "sometimes",
        "always",
        "never",
        "let",
        "use",
        "call",
        "see",
        "check",
        "contact",
        "run",
        "try",
        "example",
        "remember",
        "important",
        "warning",
        "tip",
        "ok",
        "okay",
        "thanks",
        "thank",
        "hi",
        "hello",
        "dear",
        "sorry",
    ];
    let norm = fold(word);
    norm == "i"
        || is_stopword(&norm)
        || is_weekday_or_month(&norm)
        || OPENERS.contains(&norm.as_str())
}

fn is_weekday_or_month(word: &str) -> bool {
    const CALENDAR: [&str; 19] = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    CALENDAR.contains(&word)
}

/// All word n-grams (1..=max_n) of `text` as (norm_name(ngram), start, end)
/// byte offsets, for exact alias lookup. Skips n-grams that are only stopwords.
pub fn ngrams(text: &str, max_n: usize) -> Vec<(String, usize, usize)> {
    if max_n == 0 {
        return Vec::new();
    }
    let words = ngram_words(text);
    let mut out = Vec::new();
    for start in 0..words.len() {
        for n in 1..=max_n {
            if start + n > words.len() {
                break;
            }
            let s = words[start].0;
            let e = words[start + n - 1].1;
            let slice = &text[s..e];
            let toks = tokens(slice);
            if toks.is_empty() || toks.iter().all(|t| is_stopword(t)) {
                continue;
            }
            let norm = norm_name(slice);
            if !norm.is_empty() {
                out.push((norm, s, e));
            }
        }
    }
    out
}

/// Word spans for n-grams: alphanumeric runs plus inner apostrophes, hyphens
/// and dots.
fn ngram_words(text: &str) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut words = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (start, c) = chars[i];
        if !c.is_alphanumeric() {
            i += 1;
            continue;
        }
        let mut end = start + c.len_utf8();
        let mut j = i + 1;
        while j < chars.len() {
            let (pos, ch) = chars[j];
            if ch.is_alphanumeric()
                || (matches!(ch, '\'' | '’' | '-' | '.')
                    && chars
                        .get(j + 1)
                        .is_some_and(|&(_, next)| next.is_alphanumeric()))
            {
                end = pos + ch.len_utf8();
                j += 1;
            } else {
                break;
            }
        }
        words.push((start, end));
        i = j;
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::norm_name;

    const PARAGRAPH: &str = "Yesterday Alice and Bob met at New York City. They discussed the \
        \"Old Bridge\" project. Also, ping @alice_dev or follow #rustlang. See \
        https://example.com/docs/v1 for details. The config lives in Cargo.toml and the tool \
        sits in /usr/local/bin/tool.sh; start it with ./run.sh. Call get_user_name and \
        std::fs::read, or getUserName. We deployed Postgres 16 on Monday in January. I checked \
        the API.";

    fn find<'a>(cands: &'a [Candidate], name: &str) -> &'a Candidate {
        cands
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("missing {name:?} in {cands:#?}"))
    }

    #[test]
    fn heuristic_entities_finds_every_kind() {
        let c = heuristic_entities(PARAGRAPH);
        for (name, hint) in [
            ("Alice", "proper"),
            ("Bob", "proper"),
            ("New York City", "proper"),
            ("Old Bridge", "quoted"),
            ("@alice_dev", "handle"),
            ("rustlang", "hashtag"),
            ("https://example.com/docs/v1", "url"),
            ("Cargo.toml", "path"),
            ("/usr/local/bin/tool.sh", "path"),
            ("./run.sh", "path"),
            ("get_user_name", "identifier"),
            ("std::fs::read", "identifier"),
            ("getUserName", "identifier"),
            ("Postgres 16", "version"),
            ("API", "proper"),
        ] {
            assert_eq!(find(&c, name).hint, hint, "hint for {name:?}");
        }
    }

    #[test]
    fn heuristic_entities_offsets_slice_the_input() {
        for cand in heuristic_entities(PARAGRAPH) {
            assert_eq!(
                &PARAGRAPH[cand.start..cand.end],
                cand.name,
                "offsets for {:?}",
                cand.name
            );
        }
    }

    #[test]
    fn heuristic_entities_dedupes_and_orders() {
        let c = heuristic_entities(PARAGRAPH);
        let old_bridge = c
            .iter()
            .filter(|x| norm_name(&x.name) == "old bridge")
            .count();
        assert_eq!(old_bridge, 1, "quoted and proper span must dedupe");
        assert!(c.windows(2).all(|w| w[0].start <= w[1].start));
        assert_eq!(c.first().map(|x| x.name.as_str()), Some("Alice"));
        assert_eq!(find(&c, "Old Bridge").hint, "quoted");
    }

    #[test]
    fn heuristic_entities_skips_common_words_and_calendar_names() {
        let c = heuristic_entities(PARAGRAPH);
        for bad in [
            "Yesterday",
            "They",
            "Also",
            "See",
            "The",
            "Call",
            "We",
            "I",
            "Monday",
            "January",
            "project",
            "details",
            "config",
        ] {
            let n = norm_name(bad);
            assert!(
                c.iter().all(|x| norm_name(&x.name) != n),
                "{bad:?} leaked into {c:#?}"
            );
        }
    }

    #[test]
    fn ngrams_skip_stopword_only_spans() {
        let text = "New York City is in the United States of America";
        let g = ngrams(text, 4);
        for (norm, s, e) in &g {
            assert_eq!(norm_name(&text[*s..*e]), *norm);
            assert!(text.is_char_boundary(*s) && text.is_char_boundary(*e));
        }
        let has = |n: &str| g.iter().any(|(x, _, _)| x == n);
        assert!(has("new york city"));
        assert!(has("united states of america"));
        assert!(has("york city"));
        assert!(!has("is in"));
        assert!(!has("in the"));
        assert!(!has("the"));
        let (_, s, e) = g
            .iter()
            .find(|(x, _, _)| x == "united states of america")
            .unwrap();
        assert_eq!(&text[*s..*e], "United States of America");
    }

    #[test]
    fn ngrams_keep_inner_apostrophes_in_offsets() {
        let text = "Mom's house";
        let g = ngrams(text, 2);
        assert!(g
            .iter()
            .any(|(n, s, e)| n == "mom" && &text[*s..*e] == "Mom's"));
        assert!(g.iter().any(|(n, _, _)| n == "moms house"));
        assert!(g
            .iter()
            .any(|(n, s, e)| n == "house" && &text[*s..*e] == "house"));
    }
}
