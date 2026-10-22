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
    let duplicate: Option<i64> = db
        .sql()
        .query_row(
            "SELECT id FROM memories WHERE content_hash=?1 AND expired_at IS NULL LIMIT 1",
            [&hash],
            |r| r.get(0),
        )
        .optional()?;
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
        let duplicate_of = duplicate;
        db.sql().execute(
            "INSERT OR IGNORE INTO meta(key,value) VALUES('embedder',?1)",
            [emb.name()],
        )?;
        let id = if let Some(id) = duplicate {
            db.sql().execute(
                "UPDATE memories SET access_count=access_count+1,last_access=?1 WHERE id=?2",
                params![now, id],
            )?;
            id
        } else {
            db.sql().execute("INSERT INTO memories(content,kind,tags,importance,scope,source,session,created_at,valid_from,valid_to,content_hash) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![content,kind,tags,importance,inp.scope.unwrap_or_default(),inp.source.unwrap_or_default(),inp.session.unwrap_or_default(),now,valid_from,valid_to,hash])?;
            let id = db.sql().last_insert_rowid();
            db.sql().execute(
                "INSERT INTO memories_vec(rowid,embedding,live) VALUES (?1,?2,1)",
                params![id, vec_blob(&vectors[0])],
            )?;
            kg::project_memory(db, id, &kind)?;
            id
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
        for (ri, relation) in inp.relations.iter().enumerate() {
            let src = resolve_endpoint(
                db,
                &relation.source,
                &mut ids,
                now,
                cfg,
                endpoint_vector(&relation.source),
            )?;
            let dst = resolve_endpoint(
                db,
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
            let fact = facts[ri].clone();
            if relation.exclusive || predicate_exclusive {
                let mut st = db.sql().prepare(
                    "SELECT id,valid_from,valid_to FROM relations
                     WHERE src=?1 AND predicate_id=?2 AND dst!=?3 AND expired_at IS NULL",
                )?;
                let olds: Vec<(i64, i64, Option<i64>)> = st
                    .query_map(params![src, pid, dst], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                let mut later: Option<i64> = None;
                for (old, old_from, old_to) in olds {
                    if old_from == rf {
                        // Equal start: the newer write wins.
                        db.sql().execute(
                            "UPDATE relations SET valid_to=?1, expired_at=?2 WHERE id=?3",
                            params![rf, now, old],
                        )?;
                        invalidated.push(old);
                    } else if old_from < rf && old_to.is_none_or(|to| to > rf) {
                        // Older and still true at the new start: it ends there.
                        db.sql().execute(
                            "UPDATE relations SET valid_to=?1 WHERE id=?2",
                            params![rf, old],
                        )?;
                        invalidated.push(old);
                    } else if old_from > rf {
                        // Backfill: the new fact is history up to the later one.
                        later = Some(later.map_or(old_from, |x| x.min(old_from)));
                    }
                }
                if let Some(later) = later {
                    new_valid_to = Some(new_valid_to.map_or(later, |to| to.min(later)));
                }
            }
            let existing: Option<(i64, i64)> = db
                .sql()
                .query_row(
                    "SELECT id,evidence_count FROM relations
                     WHERE src=?1 AND predicate_id=?2 AND dst=?3
                       AND valid_to IS NULL AND expired_at IS NULL",
                    params![src, pid, dst],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let rid = if let Some((rid, _count)) = existing {
                db.sql().execute(
                    "UPDATE relations SET evidence_count=evidence_count+1,
                     weight=evidence_count+1, valid_from=MIN(valid_from,?1) WHERE id=?2",
                    params![rf, rid],
                )?;
                db.sql().execute(
                    "INSERT OR IGNORE INTO relation_evidence(relation_id,memory_id) VALUES(?1,?2)",
                    params![rid, id],
                )?;
                rid
            } else {
                db.sql().execute(
                    "INSERT INTO relations(src,dst,predicate_id,fact,weight,evidence_count,valid_from,valid_to,created_at)
                     VALUES (?1,?2,?3,?4,1,1,?5,?6,?7)",
                    params![src, dst, pid, fact, rf, new_valid_to, now],
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
                rid
            };
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
        Ok(RememberOutput{id,duplicate_of,near_duplicate:near,entities:resolutions,relations:rel_results,invalidated_relations:invalidated,superseded_memories:supersedes,hints:if content.len()>2000{vec!["long content: consider splitting into atomic memories".into()]}else{Vec::new()}})
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

fn resolve_endpoint(
    db: &Db,
    name: &str,
    ids: &mut HashMap<String, i64>,
    now: i64,
    cfg: &Config,
    embedding: &[f32],
) -> Result<i64> {
    let norm = text::norm_name(name);
    if let Some(id) = ids.get(&norm) {
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
    for (other, _name, score) in &resolved.similar {
        write_similar(db, resolved.id, *other, *score)?;
    }
    Ok(resolved.id)
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
