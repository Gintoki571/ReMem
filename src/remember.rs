use crate::{
    db::{vec_blob, Db},
    embed::{Embedder, Mode},
    engine::{Config, EntityIn, EntityResolution, RelationResult, RememberInput, RememberOutput},
    error::Result,
    extract, kg, resolve,
    temporal::{self, Clock},
    text,
};
use rusqlite::{params, OptionalExtension};
use std::collections::{HashMap, HashSet};

/// Conservative negation cue for one relation: `text` must name the target,
/// and a cue must sit within 4 words before the nearest predicate verb that
/// precedes it ("Alice no longer likes jazz"). Anything else is positive.
fn infer_negated(text: &str, predicate: &str, target: &str) -> bool {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_lowercase()
        })
        .collect();
    let target: Vec<String> = target.split_whitespace().map(str::to_lowercase).collect();
    let Some(verb) = predicate.split_whitespace().next() else {
        return false;
    };
    // "like" matches likes/liked, "live" lives/lived, "work" works/worked.
    let stem = verb.trim_end_matches('e');
    let Some(tpos) = (0..words.len()).find(|&i| words[i..].starts_with(&target)) else {
        return false;
    };
    let Some(vpos) = (0..tpos).rev().find(|&i| words[i].starts_with(stem)) else {
        return false;
    };
    let window = &words[vpos.saturating_sub(4)..vpos];
    let has = |w: &str| window.iter().any(|x| x == w);
    let pair = |a: &str, b: &str| window.windows(2).any(|p| p[0] == a && p[1] == b);
    let mut cues = [
        "not", "never", "stopped", "ceased", "doesn't", "didn't", "don't", "isn't", "wasn't",
        "won't", "nor",
    ]
    .iter()
    .any(|c| has(c))
        || pair("no", "longer")
        || pair("no", "more")
        || pair("does", "not")
        || pair("did", "not");
    if pair("not", "only") || pair("never", "stopped") {
        cues = false;
    }
    cues
}

pub fn check_embedder(db: &Db, emb: &dyn Embedder) -> Result<()> {
    let found: Option<String> = db
        .sql()
        .query_row("SELECT value FROM meta WHERE key='embedder'", [], |r| {
            r.get(0)
        })
        .optional()?;
    if let Some(found) = found {
        if found != emb.name() {
            return Err(crate::err!("this database was built with embedder '{}' but '{}' is active; use the same embedder or another --db", found, emb.name()));
        }
    }
    Ok(())
}

pub fn remember(
    db: &Db,
    emb: &dyn Embedder,
    cfg: &Config,
    clock: Clock,
    inp: RememberInput,
) -> Result<RememberOutput> {
    check_embedder(db, emb)?;
    let content = inp.content.trim().to_string();
    if content.is_empty() {
        return Err(crate::err!("content must not be empty"));
    }
    let kind = inp.kind.unwrap_or_else(|| "note".into());
    if !crate::engine::KINDS.contains(&kind.as_str()) {
        return Err(crate::err!(
            "invalid kind '{}'; valid kinds: {}",
            kind,
            crate::engine::KINDS.join(", ")
        ));
    }
    let valid_from = inp
        .valid_from
        .as_deref()
        .map(|s| {
            temporal::parse_instant(s, &clock).ok_or_else(|| {
                crate::err!(
                    "invalid valid_from '{}'; use ISO date, unix seconds, or 'yesterday'",
                    s
                )
            })
        })
        .transpose()?
        .unwrap_or(clock.now);
    let valid_to = inp
        .valid_to
        .as_deref()
        .map(|s| {
            temporal::parse_instant(s, &clock).ok_or_else(|| {
                crate::err!(
                    "invalid valid_to '{}'; use ISO date, unix seconds, or 'tomorrow'",
                    s
                )
            })
        })
        .transpose()?;
    if valid_to.is_some_and(|v| v <= valid_from) {
        return Err(crate::err!("valid_to must be after valid_from"));
    }
    let importance = inp
        .importance
        .filter(|x| !x.is_nan())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    let mut tags: Vec<String> = inp
        .tags
        .iter()
        .chain(inp.keywords.iter())
        .map(|x| text::fold(x))
        .filter(|x| !x.is_empty())
        .collect();
    tags.sort();
    tags.dedup();
    let tags = tags.join(" ");
    let mut entities = inp.entities.clone();
    if entities.is_empty() {
        entities = extract::heuristic_entities(&content)
            .into_iter()
            .map(|c| EntityIn {
                name: c.name,
                kind: None,
                description: None,
                aliases: Vec::new(),
                same_as: None,
            })
            .collect();
    }
    let cards: Vec<String> = entities
        .iter()
        .map(|e| {
            format!(
                "{} ({}): {}",
                e.name,
                e.kind.as_deref().unwrap_or(""),
                e.description.as_deref().unwrap_or("")
            )
        })
        .collect();
    let facts: Vec<String> = inp
        .relations
        .iter()
        .map(|r| {
            r.fact
                .clone()
                .unwrap_or_else(|| format!("{} {} {}", r.source, r.predicate, r.target))
        })
        .collect();
    // Every relation endpoint that is not one of this memory's entities gets
    // its own bare-name vector. Indices are explicit so no vector is ever
    // reused for another text.
    let entity_norms: HashSet<String> = entities.iter().map(|e| text::norm_name(&e.name)).collect();
    let mut extra_names: Vec<String> = Vec::new();
    let mut extra_index: HashMap<String, usize> = HashMap::new();
    for relation in &inp.relations {
        for name in [&relation.source, &relation.target] {
            let norm = text::norm_name(name);
            if !entity_norms.contains(&norm) && !extra_index.contains_key(&norm) {
                extra_index.insert(norm, extra_names.len());
                extra_names.push(name.clone());
            }
        }
    }
    let mut embed_texts: Vec<&str> =
        Vec::with_capacity(1 + cards.len() + facts.len() + extra_names.len());
    embed_texts.push(&content);
    let card_base = 1;
    embed_texts.extend(cards.iter().map(String::as_str));
    let fact_base = card_base + cards.len();
    embed_texts.extend(facts.iter().map(String::as_str));
    let extra_base = fact_base + facts.len();
    embed_texts.extend(extra_names.iter().map(String::as_str));
    let vectors = emb.embed(&embed_texts, Mode::Document)?;
    if vectors.len() != embed_texts.len() {
        return Err(crate::err!(
            "embedder returned {} vectors for {} texts",
            vectors.len(),
            embed_texts.len()
        ));
    }
    let endpoint_vector = |name: &str| -> &[f32] {
        let norm = text::norm_name(name);
        if let Some(pos) = entities
            .iter()
            .position(|e| text::norm_name(&e.name) == norm)
        {
            &vectors[card_base + pos]
        } else {
            let index = extra_index[&norm];
            &vectors[extra_base + index]
        }
    };
    let hash = text::content_hash(&kind, &content);
    let near = knn_memory(db, &vectors[0], cfg.near_dup)?;
    let supersedes: Vec<i64> = inp
        .supersedes
        .iter()
        .map(|s| {
            crate::engine::parse_id(s, 'm')
                .ok_or_else(|| crate::err!("invalid supersedes id '{s}'"))
        })
        .collect::<Result<_>>()?;
    for id in &supersedes {
        let exists: bool = db.sql().query_row(
            "SELECT EXISTS(SELECT 1 FROM memories WHERE id=?1 AND expired_at IS NULL)",
            [id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(crate::err!(
                "supersedes target m{} is unknown or already expired",
                id
            ));
        }
    }
    db.write(|db| {
        let now = clock.now;
        // The duplicate decision is made under the write lock: a read outside
        // it races with concurrent writers (the unique index is the backstop).
        let mut duplicate_of = live_duplicate(db, &hash, &kind, &content)?;
        let stored_hash = if duplicate_of.is_some() {
            hash.clone()
        } else {
            // A live row with this hash but different text is a hash collision;
            // salt the key so both memories can be stored.
            free_hash(db, &hash)?
        };
        db.sql().execute(
            "INSERT OR IGNORE INTO meta(key,value) VALUES('embedder',?1)",
            [emb.name()],
        )?;
        let id = if let Some(id) = duplicate_of {
            touch_memory(db, id, now)?;
            id
        } else {
            let inserted = db.sql().execute("INSERT INTO memories(content,kind,tags,importance,scope,source,session,created_at,valid_from,valid_to,content_hash) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![content,kind,tags,importance,inp.scope.unwrap_or_default(),inp.source.unwrap_or_default(),inp.session.unwrap_or_default(),now,valid_from,valid_to,stored_hash]);
            match inserted {
                Ok(_) => {
                    let id = db.sql().last_insert_rowid();
                    db.sql().execute(
                        "INSERT INTO memories_vec(rowid,embedding,live) VALUES (?1,?2,1)",
                        params![id, vec_blob(&vectors[0])],
                    )?;
                    kg::project_memory(db, id, &kind)?;
                    id
                }
                Err(e) if crate::db::is_unique_violation(&e) => {
                    // An identical writer won the race; reuse its row.
                    let id: i64 = db.sql().query_row(
                        "SELECT id FROM memories WHERE content_hash=?1 AND expired_at IS NULL",
                        [&stored_hash],
                        |r| r.get(0),
                    )?;
                    touch_memory(db, id, now)?;
                    duplicate_of = Some(id);
                    id
                }
                Err(e) => return Err(e.into()),
            }
        };
        let mut resolutions = Vec::new();
        let mut ids: HashMap<String, i64> = HashMap::new();
        for (i, entity) in entities.iter().enumerate() {
            let resolved = resolve::resolve_entity(db, entity, &vectors[card_base + i], now, cfg)?;
            let eid = resolved.id;
            ids.insert(text::norm_name(&entity.name), eid);
            kg::project_entity_from_db(db, eid)?;
            add_mention(db, id, eid, now)?;
            for (other, _name, score) in &resolved.similar {
                write_similar(db, eid, *other, *score)?;
            }
            resolutions.push(EntityResolution {
                name: entity.name.clone(),
                id: eid,
                status: resolved.status,
                similar: resolved.similar,
            });
        }
        // Gazetteer: link known aliases that appear as whole-word n-grams in
        // the content (never agent-supplied entities, which are already in).
        let mut seen: HashSet<String> = HashSet::new();
        for (norm, _, _) in extract::ngrams(&content, 4) {
            if norm.chars().count() < 3 || !seen.insert(norm.clone()) {
                continue;
            }
            let mut st = db
                .sql()
                .prepare("SELECT DISTINCT entity_id FROM entity_aliases WHERE alias_norm=?1")?;
            let matches: Vec<i64> = st
                .query_map([&norm], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<_>>()?;
            if matches.len() == 1 {
                let eid = matches[0];
                kg::project_entity_from_db(db, eid)?;
                add_mention(db, id, eid, now)?;
            }
        }
        let mut rel_results = Vec::new();
        let mut invalidated = Vec::new();
        let mut conflicts = Vec::new();
        let mut related = Vec::new();
        if inp.relations.is_empty() && !ids.is_empty() {
            let entity_ids: Vec<i64> = ids.values().copied().collect();
            let mut st = db.sql().prepare(
                "SELECT rowid,1-distance FROM memories_vec
                 WHERE embedding MATCH ?1 AND k=10 AND live=1 ORDER BY distance",
            )?;
            for row in st.query_map([vec_blob(&vectors[0])], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f32>(1)?))
            })? {
                let (mid, cosine) = row?;
                if cosine < cfg.update_candidate || cosine >= cfg.near_dup || mid == id {
                    continue;
                }
                let shared: bool = db.sql().query_row(
                    "SELECT EXISTS(SELECT 1 FROM mentions WHERE memory_id=?1 AND entity_id IN (SELECT value FROM json_each(?2)))",
                    rusqlite::params![mid, serde_json::to_string(&entity_ids)?],
                    |r| r.get(0),
                )?;
                if shared {
                    let snippet: String = db.sql().query_row(
                        "SELECT content FROM memories WHERE id=?1", [mid], |r| r.get(0)
                    )?;
                    related.push(crate::engine::RelatedMemory { memory_id: mid, cosine, snippet });
                    if related.len() == 3 { break; }
                }
            }
        }
        for (ri, relation) in inp.relations.iter().enumerate() {
            let src = resolve_endpoint(
                db,
                id,
                &relation.source,
                &mut ids,
                now,
                cfg,
                endpoint_vector(&relation.source),
            )?;
            let dst = resolve_endpoint(
                db,
                id,
                &relation.target,
                &mut ids,
                now,
                cfg,
                endpoint_vector(&relation.target),
            )?;
            let (pid, predicate, predicate_exclusive) = resolve::resolve_predicate(
                db,
                &relation.predicate,
                &vectors[fact_base + ri],
                relation.exclusive,
                cfg,
            )?;
            let rf = relation
                .valid_from
                .as_deref()
                .map(|s| {
                    temporal::parse_instant(s, &clock)
                        .ok_or_else(|| crate::err!("invalid relation valid_from '{s}'"))
                })
                .transpose()?
                .unwrap_or(valid_from);
            let mut new_valid_to = relation
                .valid_to
                .as_deref()
                .map(|x| {
                    temporal::parse_instant(x, &clock)
                        .ok_or_else(|| crate::err!("invalid relation valid_to '{x}'"))
                })
                .transpose()?;
            let mut fact = facts[ri].clone();
            let polarity = if relation.negated.unwrap_or_else(|| {
                infer_negated(
                    relation.fact.as_deref().unwrap_or(&content),
                    &predicate,
                    &relation.target,
                )
            }) {
                0
            } else {
                1
            };
            if polarity == 0 && relation.fact.is_none() {
                fact = format!("{} NOT {} {}", relation.source, relation.predicate, relation.target);
            }
            // Relations this one ends; each gets ended_by = this relation.
            let mut ended_here: Vec<i64> = Vec::new();
            // The later relation this backfilled one is history up to.
            let mut ended_by_later: Option<i64> = None;
            if new_valid_to.is_some_and(|to| to <= rf) {
                return Err(crate::err!(
                    "relation '{fact}': valid_to must be after valid_from"
                ));
            }
            if polarity == 1
                && relation.exclusive != Some(false)
                && (relation.exclusive == Some(true) || predicate_exclusive)
            {
                let mut st = db.sql().prepare(
                    "SELECT id,valid_from,valid_to FROM relations
                     WHERE src=?1 AND predicate_id=?2 AND dst!=?3 AND polarity=1 AND expired_at IS NULL",
                )?;
                let olds: Vec<(i64, i64, Option<i64>)> = st
                    .query_map(params![src, pid, dst], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                let mut later: Option<(i64, i64)> = None;
                for (old, old_from, old_to) in olds {
                    if old_from == rf {
                        // Equal start: the newer write wins.
                        db.sql().execute(
                            "UPDATE relations SET valid_to=?1, expired_at=?2 WHERE id=?3",
                            params![rf, now, old],
                        )?;
                        ended_here.push(old);
                    } else if old_from < rf && old_to.is_none_or(|to| to > rf) {
                        // Older and still true at the new start: it ends there.
                        db.sql().execute(
                            "UPDATE relations SET valid_to=?1 WHERE id=?2",
                            params![rf, old],
                        )?;
                        ended_here.push(old);
                    } else if old_from > rf {
                        // Backfill: the new fact is history up to the later one.
                        if later.is_none_or(|(from, _)| old_from < from) {
                            later = Some((old_from, old));
                        }
                    }
                }
                if let Some((later, by)) = later {
                    if new_valid_to.is_none_or(|to| to > later) {
                        new_valid_to = Some(later);
                        ended_by_later = Some(by);
                    }
                }
            }
            if relation.exclusive.is_none() && !predicate_exclusive && polarity == 1 {
                let mut st = db.sql().prepare(
                    "SELECT r.id,r.fact,r.valid_from FROM relations r
                     WHERE r.src=?1 AND r.predicate_id=?2 AND r.dst!=?3
                       AND r.polarity=1 AND r.expired_at IS NULL
                       AND (r.valid_to IS NULL OR r.valid_to>?4) LIMIT 5",
                )?;
                for row in st.query_map(params![src, pid, dst, rf], |r| {
                    Ok(crate::engine::Conflict {
                        relation_id: r.get(0)?,
                        fact: r.get(1)?,
                        valid_from: r.get(2)?,
                    })
                })? {
                    conflicts.push(row?);
                }
            }
            let existing: Option<(i64, i64, Option<i64>)> = db
                .sql()
                .query_row(
                    "SELECT id,evidence_count,valid_to FROM relations
                     WHERE src=?1 AND predicate_id=?2 AND dst=?3 AND polarity=?4
                       AND expired_at IS NULL
                       AND (valid_to IS NULL OR valid_to>=?5)
                       AND valid_from<=COALESCE(?6,valid_from) LIMIT 1",
                    params![src, pid, dst, polarity, rf, new_valid_to],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let rid = if let Some((rid, _count, _old_to)) = existing {
                db.sql().execute(
                    "UPDATE relations SET evidence_count=evidence_count+1,
                     weight=evidence_count+1, valid_from=MIN(valid_from,?1),
                     valid_to=CASE WHEN valid_to IS NULL THEN NULL
                                   WHEN ?2 IS NULL THEN CASE WHEN ?1>=valid_to THEN NULL ELSE valid_to END
                                   ELSE MAX(valid_to,?2) END
                     WHERE id=?3",
                    params![rf, new_valid_to, rid],
                )?;
                db.sql().execute(
                    "INSERT OR IGNORE INTO relation_evidence(relation_id,memory_id) VALUES(?1,?2)",
                    params![rid, id],
                )?;
                rid
            } else {
                db.sql().execute(
                    "INSERT INTO relations(src,dst,predicate_id,fact,weight,evidence_count,valid_from,valid_to,created_at,polarity)
                     VALUES (?1,?2,?3,?4,1,1,?5,?6,?7,?8)",
                    params![src, dst, pid, fact, rf, new_valid_to, now, polarity],
                )?;
                let rid = db.sql().last_insert_rowid();
                db.sql().execute(
                    "INSERT INTO relation_evidence(relation_id,memory_id) VALUES(?1,?2)",
                    params![rid, id],
                )?;
                db.sql().execute(
                    "INSERT INTO relations_fts(rowid,fact,predicate) VALUES (?1,?2,?3)",
                    params![rid, fact, predicate],
                )?;
                db.sql().execute(
                    "INSERT INTO relations_vec(rowid,embedding) VALUES(?1,?2)",
                    params![rid, vec_blob(&vectors[fact_base + ri])],
                )?;
                // Polarity flip: the same triple asserted with the opposite
                // polarity ends the older live assertion (both stay history).
                let opposite: Vec<i64> = {
                    let mut st = db.sql().prepare(
                        "SELECT id FROM relations WHERE src=?1 AND dst=?2 AND predicate_id=?3
                         AND polarity=?4 AND valid_from<?5 AND expired_at IS NULL
                         AND (valid_to IS NULL OR valid_to>?5)",
                    )?;
                    let rows = st.query_map(params![src, dst, pid, 1 - polarity, rf], |r| r.get(0))?;
                    rows.collect::<rusqlite::Result<_>>()?
                };
                for old in opposite {
                    db.sql().execute(
                        "UPDATE relations SET valid_to=?1 WHERE id=?2",
                        params![rf, old],
                    )?;
                    ended_here.push(old);
                }
                rid
            };
            for old in &ended_here {
                db.sql().execute(
                    "UPDATE relations SET ended_by=?1 WHERE id=?2",
                    params![rid, old],
                )?;
            }
            if let Some(by) = ended_by_later {
                db.sql().execute(
                    "UPDATE relations SET ended_by=?1 WHERE id=?2 AND valid_to IS NOT NULL",
                    params![by, rid],
                )?;
            }
            invalidated.extend(ended_here);
            kg::project_relation(db, rid, src, dst, &predicate)?;
            rel_results.push(RelationResult {
                id: rid,
                fact,
                status: if existing.is_some() {
                    "merged".into()
                } else {
                    "new".into()
                },
            });
        }

        for target in &supersedes {
            // Relations this memory was the only live evidence for were true
            // until the new memory's world-time starts; they end there and stay
            // true-in-the-past (expired_at stays NULL).
            let ending = relations_losing_their_last_evidence(db, *target)?;
            for rid in &ending {
                db.sql().execute(
                    "UPDATE relations SET valid_to=MIN(COALESCE(valid_to,?1),?1)
                     WHERE id=?2 AND expired_at IS NULL",
                    params![valid_from, rid],
                )?;
            }

            db.sql().execute(
                "UPDATE memories SET valid_to=COALESCE(valid_to,?1),expired_at=?2,superseded_by=?3 WHERE id=?4",
                params![valid_from, now, id, target],
            )?;
            db.sql()
                .execute("UPDATE memories_vec SET live=0 WHERE rowid=?1", [target])?;
        }
        let touched: Vec<i64> = invalidated
            .iter()
            .copied()
            .chain(rel_results.iter().map(|r| r.id))
            .collect();
        end_memories_of_ended_facts(db, &touched)?;
        kg::bump_gen(db)?;
        let mut hints = if content.len()>2000{vec!["long content: consider splitting into atomic memories".into()]}else{Vec::new()};
        if !conflicts.is_empty() {
            let c = &conflicts[0];
            hints.push(format!("r{} ({}) is also current; if the new fact replaces it call invalidate r{}, or re-send with exclusive=true", c.relation_id, c.fact, c.relation_id));
        }
        if !related.is_empty() {
            hints.push(format!("if this updates m{}, pass supersedes=[m{}]", related[0].memory_id, related[0].memory_id));
        }
        Ok(RememberOutput{id,duplicate_of,near_duplicate:near,entities:resolutions,relations:rel_results,invalidated_relations:invalidated,superseded_memories:supersedes,conflicts,related,hints})
    })
}

fn add_mention(db: &Db, memory: i64, entity: i64, now: i64) -> Result<bool> {
    let inserted = db.sql().execute(
        "INSERT OR IGNORE INTO mentions(memory_id,entity_id) VALUES (?1,?2)",
        params![memory, entity],
    )? > 0;
    if inserted {
        db.sql().execute(
            "UPDATE entities SET mention_count=mention_count+1, updated_at=?1 WHERE id=?2",
            params![now, entity],
        )?;
        kg::project_mention(db, memory, entity)?;
    }
    Ok(inserted)
}

fn write_similar(db: &Db, entity: i64, other: i64, score: f32) -> Result<()> {
    if entity == other {
        return Ok(());
    }
    let (a, b) = if entity < other {
        (entity, other)
    } else {
        (other, entity)
    };
    db.sql().execute(
        "INSERT OR REPLACE INTO similar(a,b,score) VALUES (?1,?2,?3)",
        params![a, b, score],
    )?;
    kg::project_similar(db, a, b, score)
}

/// A memory whose every evidenced relation has ended describes a past state:
/// give it valid_to = the latest relation end (if still open), so recall treats
/// it as history ("lives in Austin" once "lives in Denver" was written).
pub fn end_memories_of_ended_facts(db: &Db, relation_ids: &[i64]) -> Result<()> {
    for rid in relation_ids {
        db.sql().execute(
            "UPDATE memories SET valid_to = (
                 SELECT MAX(r.valid_to) FROM relation_evidence e JOIN relations r ON r.id = e.relation_id
                 WHERE e.memory_id = memories.id)
             WHERE valid_to IS NULL
               AND id IN (SELECT memory_id FROM relation_evidence WHERE relation_id = ?1)
               AND NOT EXISTS (
                 SELECT 1 FROM relation_evidence e JOIN relations r ON r.id = e.relation_id
                 WHERE e.memory_id = memories.id AND r.valid_to IS NULL)
               AND (SELECT MAX(r.valid_to) FROM relation_evidence e JOIN relations r ON r.id = e.relation_id
                    WHERE e.memory_id = memories.id) > valid_from",
            [rid],
        )?;
    }
    Ok(())
}

/// Resolve a relation endpoint and record that `memory` mentions it.
fn resolve_endpoint(
    db: &Db,
    memory: i64,
    name: &str,
    ids: &mut HashMap<String, i64>,
    now: i64,
    cfg: &Config,
    embedding: &[f32],
) -> Result<i64> {
    let norm = text::norm_name(name);
    if let Some(id) = ids.get(&norm) {
        add_mention(db, memory, *id, now)?;
        return Ok(*id);
    }
    let resolved = resolve::resolve_entity(
        db,
        &EntityIn {
            name: name.into(),
            ..Default::default()
        },
        embedding,
        now,
        cfg,
    )?;
    ids.insert(norm, resolved.id);
    kg::project_entity_from_db(db, resolved.id)?;
    add_mention(db, memory, resolved.id, now)?;
    for (other, _name, score) in &resolved.similar {
        write_similar(db, resolved.id, *other, *score)?;
    }
    Ok(resolved.id)
}

/// The live memory whose kind and folded text equal `kind` + `content`, if any.
/// A hash hit with different text is a collision, not a duplicate.
fn live_duplicate(db: &Db, hash: &str, kind: &str, content: &str) -> Result<Option<i64>> {
    let row: Option<(i64, String, String)> = db
        .sql()
        .query_row(
            "SELECT id,kind,content FROM memories
             WHERE content_hash=?1 AND expired_at IS NULL LIMIT 1",
            [hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let folded = text::fold(content);
    Ok(row
        .filter(|(_, k, c)| k == kind && text::fold(c) == folded)
        .map(|(id, _, _)| id))
}

/// `hash`, or a salted variant when a live row already owns it with other text.
fn free_hash(db: &Db, hash: &str) -> Result<String> {
    let mut candidate = hash.to_string();
    loop {
        let taken: Option<i64> = db
            .sql()
            .query_row(
                "SELECT 1 FROM memories WHERE content_hash=?1 AND expired_at IS NULL",
                [&candidate],
                |r| r.get(0),
            )
            .optional()?;
        if taken.is_none() {
            return Ok(candidate);
        }
        candidate.push('#');
    }
}

fn touch_memory(db: &Db, id: i64, now: i64) -> Result<()> {
    db.sql().execute(
        "UPDATE memories SET access_count=access_count+1,last_access=?1 WHERE id=?2",
        params![now, id],
    )?;
    Ok(())
}

/// Relations evidenced by `memory` for which no *other live* memory is evidence.
pub fn relations_losing_their_last_evidence(db: &Db, memory: i64) -> Result<Vec<i64>> {
    let mut st = db.sql().prepare(
        "SELECT DISTINCT e.relation_id FROM relation_evidence e
         WHERE e.memory_id=?1
           AND NOT EXISTS (
             SELECT 1 FROM relation_evidence e2 JOIN memories m ON m.id=e2.memory_id
             WHERE e2.relation_id=e.relation_id AND e2.memory_id<>?1
               AND m.expired_at IS NULL)",
    )?;
    let rows = st.query_map([memory], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Soft-retire relations: `valid_to = now` when still open, `expired_at = now`.
pub fn expire_relations(db: &Db, ids: &[i64], now: i64) -> Result<()> {
    for rid in ids {
        db.sql().execute(
            "UPDATE relations SET valid_to=COALESCE(valid_to,?1), expired_at=?1
             WHERE id=?2 AND expired_at IS NULL",
            params![now, rid],
        )?;
    }
    Ok(())
}

fn knn_memory(db: &Db, v: &[f32], threshold: f32) -> Result<Option<(i64, f32)>> {
    let row: Option<(i64, f32)> = db
        .sql()
        .query_row(
            "SELECT rowid,1-distance FROM memories_vec WHERE embedding MATCH ?1 AND k=1 AND live=1 ORDER BY distance",
            [vec_blob(v)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.filter(|(_, s)| *s >= threshold))
}

#[cfg(test)]
mod negation_tests {
    use super::infer_negated;

    #[test]
    fn negation_cues_are_anchored_on_the_target() {
        let cases = [
            ("Alice no longer likes jazz.", "like", "jazz", true),
            ("Alice doesn't like jazz", "like", "jazz", true),
            ("Alice does not like jazz", "like", "jazz", true),
            ("Bob never worked at Acme", "work at", "Acme", true),
            (
                "Bob stopped working at Acme in May",
                "work at",
                "Acme",
                true,
            ),
            (
                "Carol isn't living in Pune anymore",
                "live in",
                "Pune",
                true,
            ),
            ("Alice likes jazz.", "like", "jazz", false),
            ("Bob works at Acme, not Globex.", "work at", "Acme", false),
            (
                "Alice moved to Denver and no longer lives in Austin.",
                "live in",
                "Denver",
                false,
            ),
            (
                "Alice moved to Denver and no longer lives in Austin.",
                "live in",
                "Austin",
                true,
            ),
            (
                "Alice not only likes jazz but also blues",
                "like",
                "jazz",
                false,
            ),
            ("Alice left for Paris", "live in", "Paris", false),
            ("I have no doubt Alice likes jazz", "like", "jazz", false),
            ("Jazz is great", "like", "jazz", false),
        ];
        for (text, predicate, target, want) in cases {
            assert_eq!(
                infer_negated(text, predicate, target),
                want,
                "{text} / {target}"
            );
        }
    }
}
