//! Compact text renderers: what an AI agent reads. Ids first, dates short,
//! empty sections omitted. Dates arrive through a caller-supplied formatter so
//! the renderers stay pure and testable without a clock.

use crate::engine::{
    ChangeOutput, EntityCard, EntityOutput, FactHit, MemoryHit, OverviewOutput, RecallOutput,
    RememberOutput, Stats,
};
use crate::temporal::Clock;

/// Formats one unix timestamp for display.
pub type Fmt<'a> = &'a dyn Fn(i64) -> String;

/// A date formatter bound to `clock`; pass `&clock_fmt(&clock)` to a renderer.
pub fn clock_fmt(clock: &Clock) -> impl Fn(i64) -> String + '_ {
    move |t| crate::temporal::fmt_date(t, clock)
}

/// `[m12] fact 2025-03-02→2026-09-14 [ended] · text  (bm25#2 vec#1)`
fn memory_line(m: &MemoryHit, fmt: Fmt<'_>) -> String {
    let mut line = format!("[m{}] {} {}", m.id, m.kind, fmt(m.valid_from));
    if let Some(to) = m.valid_to {
        line.push('→');
        line.push_str(&fmt(to));
    }

    if m.ended {
        match m.superseded_by {
            Some(by) => line.push_str(&format!(" [superseded by m{by}]")),
            None => line.push_str(" [ended]"),
        }
    }
    line.push_str(" · ");
    line.push_str(&m.content);
    if !m.reasons.is_empty() {
        line.push_str("  (");
        line.push_str(&m.reasons.join(" "));
        line.push(')');
    }
    line
}

fn truncate_content(content: &str, max_chars: usize) -> String {
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    let mut out: String = content.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn timeline_memory_line(m: &MemoryHit, fmt: Fmt<'_>) -> String {
    truncate_content(&memory_line(m, fmt), 160)
}

/// `[r9] Mom —live in→ Pune (since 2026-09-14)`
fn fact_line(f: &FactHit, fmt: Fmt<'_>) -> String {
    let span = match (f.valid_to, f.ended) {
        (Some(to), true) => match &f.replaced_by {
            Some((id, fact)) => format!(
                "{} → {}, replaced by r{id}: {fact}",
                fmt(f.valid_from),
                fmt(to)
            ),
            None => format!("{} → {}, ended", fmt(f.valid_from), fmt(to)),
        },
        (Some(to), false) => format!("{} → {}", fmt(f.valid_from), fmt(to)),
        (None, true) => format!("{}, ended", fmt(f.valid_from)),
        (None, false) => format!("since {}", fmt(f.valid_from)),
    };
    let arrow = if f.polarity == 0 { "—NOT " } else { "—" };
    let suffix = f
        .current_values
        .map(|n| format!(" [1 of {n} current]"))
        .unwrap_or_default();
    format!(
        "[r{}] {} {}{}→ {}{} ({span})",
        f.id, f.source, arrow, f.predicate, f.target, suffix
    )
}

/// `[e3] Mom (person; aka mother)`
fn entity_card(c: &EntityCard) -> String {
    let mut details = Vec::new();
    if !c.kind.is_empty() {
        details.push(c.kind.clone());
    }
    if !c.aliases.is_empty() {
        details.push(format!("aka {}", c.aliases.join(", ")));
    }
    if details.is_empty() {
        format!("[e{}] {}", c.id, c.name)
    } else {
        format!("[e{}] {} ({})", c.id, c.name, details.join("; "))
    }
}

fn id_list(prefix: char, ids: &[i64]) -> String {
    ids.iter()
        .map(|id| format!("{prefix}{id}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Result of `remember`: what was stored, how it resolved, what it replaced.
pub fn render_remember(out: &RememberOutput) -> String {
    let mut s = String::new();
    match out.duplicate_of {
        Some(id) => s.push_str(&format!("duplicate of m{id} — nothing new stored\n")),
        None => s.push_str(&format!("saved m{}\n", out.id)),
    }
    if let Some((id, cos)) = out.near_duplicate {
        s.push_str(&format!(
            "near-duplicate of m{id} ({cos:.2}) — stored anyway\n"
        ));
    }
    if !out.entities.is_empty() {
        let items: Vec<String> = out
            .entities
            .iter()
            .map(|e| {
                let mut line = format!("[e{}] {} ({})", e.id, e.name, e.status);
                if !e.similar.is_empty() {
                    let sims: Vec<String> = e
                        .similar
                        .iter()
                        .map(|(id, name, cos)| format!("e{id} {name} {cos:.2}"))
                        .collect();
                    line.pop();
                    line.push_str("; similar ");
                    line.push_str(&sims.join(", "));
                    line.push(')');
                }
                line
            })
            .collect();
        s.push_str(&format!("entities: {}\n", items.join(" · ")));
    }
    if !out.relations.is_empty() {
        let items: Vec<String> = out
            .relations
            .iter()
            .map(|r| format!("[r{}] {} ({})", r.id, r.fact, r.status))
            .collect();
        s.push_str(&format!("relations: {}\n", items.join(" · ")));
    }
    if !out.invalidated_relations.is_empty() {
        s.push_str(&format!(
            "invalidated: {}\n",
            id_list('r', &out.invalidated_relations)
        ));
    }
    if !out.superseded_memories.is_empty() {
        s.push_str(&format!(
            "superseded: {}\n",
            id_list('m', &out.superseded_memories)
        ));
    }
    for c in &out.conflicts {
        s.push_str(&format!(
            "conflict: r{} {} (since {})\n",
            c.relation_id,
            c.fact,
            crate::temporal::fmt_date(c.valid_from, &Clock::system())
        ));
    }
    for r in &out.related {
        s.push_str(&format!(
            "related: m{} ({:.2}) {}\n",
            r.memory_id, r.cosine, r.snippet
        ));
    }
    for hint in &out.hints {
        s.push_str(&format!("hint: {hint}\n"));
    }
    s
}

/// Result of `recall`: memories, then facts, then entity cards.
pub fn render_recall(out: &RecallOutput, fmt: Fmt<'_>) -> String {
    let mut s = String::new();
    if let Some((start, end, text)) = &out.window {
        s.push_str(&format!(
            "window: {text} ({} → {})\n",
            fmt(*start),
            fmt(*end)
        ));
    }
    if out.memories.is_empty() {
        s.push_str(
            "no memories matched — try fewer filters, a broader query or another spelling.\n",
        );
    } else {
        let total = out.memories.len() + out.omitted;
        s.push_str(&format!(
            "memories ({} of {}, ~{}/{} tokens):\n",
            out.memories.len(),
            total,
            out.tokens_used,
            out.tokens_budget
        ));
        for m in &out.memories {
            s.push_str(&memory_line(m, fmt));
            s.push('\n');
        }
    }
    if !out.facts.is_empty() {
        s.push_str("facts:\n");
        for f in &out.facts {
            s.push_str(&fact_line(f, fmt));
            s.push('\n');
        }
    }
    if !out.entities.is_empty() {
        let cards: Vec<String> = out.entities.iter().map(entity_card).collect();
        s.push_str(&format!("entities: {}\n", cards.join(" · ")));
    }
    s
}

/// One entity: card, current and past facts, similar entities, timeline.
pub fn render_entity(out: &EntityOutput, fmt: Fmt<'_>) -> String {
    let mut s = entity_card(&out.card);
    if !out.card.description.is_empty() {
        s.push_str(" — ");
        s.push_str(&out.card.description);
    }
    s.push('\n');
    if !out.current_facts.is_empty() {
        s.push_str("current facts:\n");
        for f in &out.current_facts {
            s.push_str(&fact_line(f, fmt));
            s.push('\n');
        }
    }
    if !out.past_facts.is_empty() {
        s.push_str("past facts:\n");
        for f in &out.past_facts {
            s.push_str(&fact_line(f, fmt));
            s.push('\n');
        }
    }
    if !out.similar.is_empty() {
        let items: Vec<String> = out
            .similar
            .iter()
            .map(|(id, name, cos)| format!("[e{id}] {name} ({cos:.2})"))
            .collect();
        s.push_str(&format!("similar: {}\n", items.join(" · ")));
    }
    if !out.timeline.is_empty() {
        s.push_str("timeline:\n");
        for m in &out.timeline {
            s.push_str(&timeline_memory_line(m, fmt));
            s.push('\n');
        }
    }
    s
}

/// One line per changed row ("m12 valid_to=2026-10-01").
pub fn render_change(out: &ChangeOutput) -> String {
    if out.changed.is_empty() {
        return "nothing changed\n".to_string();
    }
    let mut s = out.changed.join("\n");
    s.push('\n');
    s
}

/// Communities with their central entities and strongest facts.
pub fn render_overview(out: &OverviewOutput, fmt: Fmt<'_>) -> String {
    if out.communities.is_empty() {
        return "no communities yet\n".to_string();
    }
    let mut s = String::new();
    for c in &out.communities {
        let names: Vec<&str> = c.entities.iter().map(|e| e.name.as_str()).collect();
        s.push_str(&format!(
            "[c{}] {} entities, score {:.2}: {}\n",
            c.id,
            c.size,
            c.score,
            names.join(", ")
        ));
        for f in &c.facts {
            s.push_str("  ");
            s.push_str(&fact_line(f, fmt));
            s.push('\n');
        }
        if let Some(m) = &c.representative {
            s.push_str("  e.g. ");
            s.push_str(&timeline_memory_line(m, fmt));
            s.push('\n');
        }
    }
    s
}

/// Two lines: counts, then storage and model.
pub fn render_stats(st: &Stats) -> String {
    format!(
        "memories {} (live {}) · entities {} · relations {} (live {}) · similar {} · predicates {}\n\
         db {} · embedder {} · neocortex {}\n",
        st.memories,
        st.live_memories,
        st.entities,
        st.relations,
        st.live_relations,
        st.similar_edges,
        st.predicates,
        bytes(st.db_bytes),
        st.embedder,
        st.version
    )
}

fn bytes(n: i64) -> String {
    let n = n.max(0) as f64;
    if n < 1024.0 {
        format!("{n:.0} B")
    } else if n < 1024.0 * 1024.0 {
        format!("{:.1} KiB", n / 1024.0)
    } else {
        format!("{:.1} MiB", n / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Community, EntityResolution, RelationResult};

    fn fake(t: i64) -> String {
        format!("D{t}")
    }

    fn hit(
        id: i64,
        kind: &str,
        content: &str,
        from: i64,
        to: Option<i64>,
        ended: bool,
    ) -> MemoryHit {
        MemoryHit {
            id,
            kind: kind.to_string(),
            content: content.to_string(),
            valid_from: from,
            valid_to: to,
            ended,
            superseded_by: None,
            tags: Vec::new(),
            score: 1.0,
            reasons: Vec::new(),
        }
    }

    fn fact(id: i64, target: &str, from: i64, to: Option<i64>, ended: bool) -> FactHit {
        FactHit {
            id,
            source: "Mom".to_string(),
            predicate: "live in".to_string(),
            target: target.to_string(),
            fact: format!("Mom lives in {target}"),
            valid_from: from,
            valid_to: to,
            ended,
            polarity: 1,
            current_values: None,
            replaced_by: None,
        }
    }

    fn card(id: i64, name: &str, kind: &str, aliases: &[&str]) -> EntityCard {
        EntityCard {
            id,
            name: name.to_string(),
            kind: kind.to_string(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            description: String::new(),
        }
    }

    #[test]
    fn recall_layout_matches_the_design() {
        let mut first = hit(
            42,
            "event",
            "Mom moved from Delhi to Pune.",
            100,
            None,
            false,
        );
        first.reasons = vec!["bm25#2".into(), "vec#1".into(), "graph#1".into()];
        let mut second = hit(12, "fact", "Mom lives in Delhi.", 50, Some(100), true);
        second.reasons = vec!["graph#2".into()];
        let out = RecallOutput {
            memories: vec![first, second],
            facts: vec![
                fact(9, "Pune", 100, None, false),
                fact(4, "Delhi", 50, Some(100), true),
            ],
            entities: vec![
                card(3, "Mom", "person", &["mother"]),
                card(8, "Pune", "place", &[]),
            ],
            window: None,
            candidates: 23,
            tokens_used: 410,
            tokens_budget: 1200,
            omitted: 21,
        };
        assert_eq!(
            render_recall(&out, &fake),
            "memories (2 of 23, ~410/1200 tokens):\n\
             [m42] event D100 · Mom moved from Delhi to Pune.  (bm25#2 vec#1 graph#1)\n\
             [m12] fact D50→D100 [ended] · Mom lives in Delhi.  (graph#2)\n\
             facts:\n\
             [r9] Mom —live in→ Pune (since D100)\n\
             [r4] Mom —live in→ Delhi (D50 → D100, ended)\n\
             entities: [e3] Mom (person; aka mother) · [e8] Pune (place)\n"
        );
    }

    #[test]
    fn recall_shows_the_window_and_empty_state() {
        let mut out = RecallOutput {
            memories: Vec::new(),
            facts: Vec::new(),
            entities: Vec::new(),
            window: Some((10, 20, "last week".to_string())),
            candidates: 0,
            tokens_used: 0,
            tokens_budget: 1200,
            omitted: 0,
        };
        let text = render_recall(&out, &fake);
        assert!(
            text.starts_with("window: last week (D10 → D20)\n"),
            "{text}"
        );
        assert!(text.contains("no memories matched"), "{text}");
        assert!(
            text.contains("broaden") || text.contains("broader"),
            "{text}"
        );

        out.window = None;
        let text = render_recall(&out, &fake);
        assert!(!text.contains("window:"), "{text}");
    }

    #[test]
    fn recall_marks_superseded_memories() {
        let mut m = hit(7, "fact", "Old truth.", 1, Some(2), true);
        m.superseded_by = Some(9);
        let out = RecallOutput {
            memories: vec![m],
            facts: Vec::new(),
            entities: Vec::new(),
            window: None,
            candidates: 1,
            tokens_used: 10,
            tokens_budget: 1200,
            omitted: 0,
        };
        assert!(render_recall(&out, &fake).contains("[superseded by m9]"));
    }

    #[test]
    fn remember_lists_resolution_and_changes() {
        let out = RememberOutput {
            id: 42,
            duplicate_of: None,
            near_duplicate: Some((7, 0.981)),
            entities: vec![
                EntityResolution {
                    name: "Mom".into(),
                    id: 3,
                    status: "new".into(),
                    similar: vec![(9, "Mother".into(), 0.81)],
                },
                EntityResolution {
                    name: "Pune".into(),
                    id: 8,
                    status: "matched".into(),
                    similar: Vec::new(),
                },
            ],
            relations: vec![RelationResult {
                id: 9,
                fact: "Mom lives in Pune".into(),
                status: "new".into(),
            }],
            invalidated_relations: vec![4, 6],
            superseded_memories: vec![12],
            conflicts: Vec::new(),
            related: Vec::new(),
            hints: vec!["content is long; atomic memories retrieve better".into()],
        };
        assert_eq!(
            render_remember(&out),
            "saved m42\n\
             near-duplicate of m7 (0.98) — stored anyway\n\
             entities: [e3] Mom (new; similar e9 Mother 0.81) · [e8] Pune (matched)\n\
             relations: [r9] Mom lives in Pune (new)\n\
             invalidated: r4, r6\n\
             superseded: m12\n\
             hint: content is long; atomic memories retrieve better\n"
        );
    }

    #[test]
    fn remember_reports_exact_duplicates() {
        let out = RememberOutput {
            id: 5,
            duplicate_of: Some(5),
            near_duplicate: None,
            entities: Vec::new(),
            relations: Vec::new(),
            invalidated_relations: Vec::new(),
            superseded_memories: Vec::new(),
            conflicts: Vec::new(),
            related: Vec::new(),
            hints: Vec::new(),
        };
        assert_eq!(
            render_remember(&out),
            "duplicate of m5 — nothing new stored\n"
        );
    }

    #[test]
    fn entity_profile_sections() {
        let out = EntityOutput {
            card: EntityCard {
                description: "the user's mother".into(),
                ..card(3, "Mom", "person", &["mother"])
            },
            current_facts: vec![fact(9, "Pune", 100, None, false)],
            past_facts: vec![fact(4, "Delhi", 50, Some(100), true)],
            similar: vec![(9, "Mother".into(), 0.81)],
            timeline: vec![hit(42, "event", "Moved to Pune.", 100, None, false)],
        };
        assert_eq!(
            render_entity(&out, &fake),
            "[e3] Mom (person; aka mother) — the user's mother\n\
             current facts:\n\
             [r9] Mom —live in→ Pune (since D100)\n\
             past facts:\n\
             [r4] Mom —live in→ Delhi (D50 → D100, ended)\n\
             similar: [e9] Mother (0.81)\n\
             timeline:\n\
             [m42] event D100 · Moved to Pune.\n"
        );
    }

    #[test]
    fn entity_timeline_truncates_long_memory_lines() {
        let out = EntityOutput {
            card: card(3, "Mom", "person", &[]),
            current_facts: Vec::new(),
            past_facts: Vec::new(),
            similar: Vec::new(),
            timeline: vec![hit(42, "note", &"x".repeat(300), 100, None, false)],
        };
        let rendered = render_entity(&out, &fake);
        let line = rendered
            .lines()
            .find(|line| line.starts_with("[m42]"))
            .unwrap();
        assert!(line.ends_with('…'));
        assert_eq!(line.chars().count(), 160);
    }

    #[test]
    fn change_is_one_line_per_item() {
        let out = ChangeOutput {
            changed: vec!["m12 valid_to=2026-10-01".into(), "r4 expired".into()],
        };
        assert_eq!(render_change(&out), "m12 valid_to=2026-10-01\nr4 expired\n");
        assert_eq!(
            render_change(&ChangeOutput {
                changed: Vec::new()
            }),
            "nothing changed\n"
        );
    }

    #[test]
    fn overview_lists_communities_and_facts() {
        let out = OverviewOutput {
            communities: vec![Community {
                id: 1,
                size: 2,
                entities: vec![card(3, "Mom", "person", &[]), card(8, "Pune", "place", &[])],
                facts: vec![fact(9, "Pune", 100, None, false)],
                score: 0.83,
                representative: None,
            }],
        };
        assert_eq!(
            render_overview(&out, &fake),
            "[c1] 2 entities, score 0.83: Mom, Pune\n  [r9] Mom —live in→ Pune (since D100)\n"
        );
    }

    #[test]
    fn stats_fit_on_two_lines() {
        let out = Stats {
            memories: 42,
            live_memories: 40,
            entities: 12,
            relations: 9,
            live_relations: 8,
            similar_edges: 3,
            predicates: 7,
            db_bytes: 128 * 1024,
            embedder: "hash".into(),
            version: "0.1.4".into(),
        };
        let text = render_stats(&out);
        assert!(text.starts_with("memories 42 (live 40)"), "{text}");
        assert!(text.contains("db 128.0 KiB"), "{text}");
        assert!(text.contains("neocortex 0.1.4"), "{text}");
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn entity_card_formatting_omits_parentheses_and_type_prefix() {
        let bare = EntityCard {
            id: 1,
            name: "SQLite".into(),
            kind: "".into(),
            aliases: vec![],
            description: "".into(),
        };
        assert_eq!(entity_card(&bare), "[e1] SQLite");

        let typed = EntityCard {
            id: 2,
            name: "Postgres".into(),
            kind: "database".into(),
            aliases: vec![],
            description: "".into(),
        };
        assert_eq!(entity_card(&typed), "[e2] Postgres (database)");

        let aliased = EntityCard {
            id: 3,
            name: "Postgres".into(),
            kind: "".into(),
            aliases: vec!["pgsql".into(), "postgres".into()],
            description: "".into(),
        };
        assert_eq!(entity_card(&aliased), "[e3] Postgres (aka pgsql, postgres)");

        let full = EntityCard {
            id: 4,
            name: "Mom".into(),
            kind: "person".into(),
            aliases: vec!["mother".into()],
            description: "".into(),
        };
        assert_eq!(entity_card(&full), "[e4] Mom (person; aka mother)");
    }
}
