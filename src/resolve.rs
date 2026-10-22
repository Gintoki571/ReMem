//! Entity and predicate resolution (design §7).
//!
//! Entities merge only on strong evidence: an exact name/alias, a lexical
//! match, an acronym match (with a cosine floor), `same_as`, or an explicit
//! merge tool. Embedding similarity alone only creates soft `similar` edges.

use crate::{
    db::{vec_blob, Db},
    embed::DIMS,
    engine::{Config, EntityIn},
    error::Result,
    text,
};
use rusqlite::{params, params_from_iter, OptionalExtension};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct Resolved {
    pub id: i64,
    pub status: String,
    pub similar: Vec<(i64, String, f32)>,
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn blob_to_vec(blob: &[u8]) -> Option<Vec<f32>> {
    if blob.len() != DIMS * 4 {
        return None;
    }
    Some(
        blob.chunks(4)
            .map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]]))
            .collect(),
    )
}

/// Cosine of `mention` against each of `ids` in one query. Missing vectors
/// score 0.
fn embeddings_for(db: &Db, ids: &[i64]) -> Result<HashMap<i64, Vec<f32>>> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!("SELECT rowid, embedding FROM entities_vec WHERE rowid IN ({placeholders})");
    let mut st = db.sql().prepare(&sql)?;
    let rows = st.query_map(params_from_iter(ids.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    for row in rows {
        let (id, blob) = row?;
        if let Some(v) = blob_to_vec(&blob) {
            out.insert(id, v);
        }
    }
    Ok(out)
}

fn best_by_cosine(db: &Db, ids: &[i64], embedding: &[f32]) -> Result<Option<i64>> {
    if ids.is_empty() {
        return Ok(None);
    }
    let vectors = embeddings_for(db, ids)?;
    let mut best: Option<(i64, f32)> = None;
    for id in ids {
        let score = vectors.get(id).map(|v| cosine(embedding, v)).unwrap_or(0.0);
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((*id, score));
        }
    }
    Ok(best.map(|(id, _)| id))
}

/// Ids whose alias is exactly `alias_norm` and whose type is compatible,
/// most-mentioned first.
fn exact_ids(db: &Db, alias_norm: &str, kind: &str) -> Result<Vec<i64>> {
    let mut st = db.sql().prepare(
        "SELECT DISTINCT e.id, e.mention_count FROM entity_aliases a
         JOIN entities e ON e.id=a.entity_id
         WHERE a.alias_norm=?1 AND (?2='' OR e.type='' OR e.type=?2)
         ORDER BY e.mention_count DESC, e.id",
    )?;
    let rows = st.query_map(params![alias_norm, kind], |r| r.get::<_, i64>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// First alias norm (name first, then agent aliases) that names an entity.
fn exact_match(db: &Db, norms: &[String], kind: &str, embedding: &[f32]) -> Result<Option<i64>> {
    for norm in norms {
        if norm.is_empty() {
            continue;
        }
        let ids = exact_ids(db, norm, kind)?;
        if let Some(id) = best_by_cosine(db, &ids, embedding)? {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

fn next_prefix(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(c) = chars.pop() {
        if let Some(next) = char::from_u32(u32::from(c) + 1) {
            chars.push(next);
            return Some(chars.into_iter().collect());
        }
    }
    None
}

/// Candidate ids: entities_vec KNN top 10, alias prefix (first 3 chars, 50
/// rows), alias equal to the mention's acronym, and entities whose acronym
/// equals the mention norm. Scores are cosines; unknown ones are computed from
/// the fetched vectors.
type Candidates = (Vec<(i64, f32)>, Vec<i64>);

fn candidates(db: &Db, norm: &str, embedding: &[f32]) -> Result<Candidates> {
    let mut knn: HashMap<i64, f32> = HashMap::new();
    let mut knn_order: Vec<i64> = Vec::new();
    {
        let mut st = db.sql().prepare(
            "SELECT rowid, 1-distance FROM entities_vec
             WHERE embedding MATCH ?1 AND k=10 ORDER BY distance",
        )?;
        let rows = st.query_map([vec_blob(embedding)], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f32>(1)?))
        })?;
        for row in rows {
            let (id, score) = row?;
            knn.insert(id, score);
            knn_order.push(id);
        }
    }
    let mut extra: HashSet<i64> = HashSet::new();
    let prefix: String = norm.chars().take(3).collect();
    if !prefix.is_empty() {
        let upper = next_prefix(&prefix);
        let mut st = db.sql().prepare(
            "SELECT entity_id FROM entity_aliases
             WHERE alias_norm >= ?1 AND (?2 IS NULL OR alias_norm < ?2) LIMIT 50",
        )?;
        let rows = st.query_map(params![prefix, upper], |r| r.get::<_, i64>(0))?;
        for row in rows {
            extra.insert(row?);
        }
    }
    if let Some(acronym) = text::acronym(norm) {
        let mut st = db
            .sql()
            .prepare("SELECT entity_id FROM entity_aliases WHERE alias_norm=?1")?;
        let rows = st.query_map([acronym.to_lowercase()], |r| r.get::<_, i64>(0))?;
        for row in rows {
            extra.insert(row?);
        }
    }
    if !norm.contains(' ') {
        // An acronym is only defined for multi-token names, so the scan is
        // limited to those and never fetches vectors.
        let mut st = db
            .sql()
            .prepare("SELECT id, norm FROM entities WHERE instr(norm, ' ')>0")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, name_norm) = row?;
            if text::acronym(&name_norm)
                .map(|a| a.to_lowercase())
                .as_deref()
                == Some(norm)
            {
                extra.insert(id);
            }
        }
    }
    let ids: Vec<i64> = knn
        .keys()
        .copied()
        .chain(extra.iter().copied().filter(|id| !knn.contains_key(id)))
        .collect();
    let missing: Vec<i64> = extra
        .iter()
        .copied()
        .filter(|id| !knn.contains_key(id))
        .collect();
    let vectors = embeddings_for(db, &missing)?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let score = knn
            .get(&id)
            .copied()
            .or_else(|| vectors.get(&id).map(|v| cosine(embedding, v)))
            .unwrap_or(0.0);
        out.push((id, score));
    }
    Ok((out, knn_order))
}

pub fn resolve_entity(
    db: &Db,
    item: &EntityIn,
    embedding: &[f32],
    now: i64,
    cfg: &Config,
) -> Result<Resolved> {
    let norm = text::norm_name(&item.name);
    if norm.is_empty() {
        return Err(crate::err!("entity name cannot be empty"));
    }
    let kind = item.kind.as_deref().unwrap_or("");

    if let Some(forced) = item.same_as.as_deref() {
        let id = resolve_same_as(db, forced)?;
        add_aliases(db, id, item, "agent")?;
        enrich(db, id, item, now)?;
        return Ok(Resolved {
            id,
            status: "same_as".into(),
            similar: Vec::new(),
        });
    }

    let mut norms = vec![norm.clone()];
    for alias in &item.aliases {
        let alias_norm = text::norm_name(alias);
        if !alias_norm.is_empty() && !norms.contains(&alias_norm) {
            norms.push(alias_norm);
        }
    }
    if let Some(id) = exact_match(db, &norms, kind, embedding)? {
        let canonical: String =
            db.sql()
                .query_row("SELECT norm FROM entities WHERE id=?1", [id], |r| r.get(0))?;
        let name_matched = canonical == norm;
        add_aliases(db, id, item, "agent")?;
        enrich(db, id, item, now)?;
        return Ok(Resolved {
            id,
            status: if name_matched { "matched" } else { "alias" }.into(),
            similar: Vec::new(),
        });
    }

    let (generated, knn_order) = candidates(db, &norm, embedding)?;
    let ids: Vec<i64> = generated.iter().map(|(id, _)| *id).collect();
    let mut details = Vec::new();
    let mut names: HashMap<i64, String> = HashMap::new();
    let mut norms: HashMap<i64, String> = HashMap::new();
    if !ids.is_empty() {
        let placeholders = vec!["?"; ids.len()].join(",");
        let sql = format!(
            "SELECT id, norm, name, type, description FROM entities WHERE id IN ({placeholders})"
        );
        let mut st = db.sql().prepare(&sql)?;
        let rows = st.query_map(params_from_iter(ids.iter()), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        for row in rows {
            let (id, entity_norm, name, entity_type, _description) = row?;
            let score = generated
                .iter()
                .find(|(candidate, _)| *candidate == id)
                .map(|(_, score)| *score)
                .unwrap_or(0.0);
            names.insert(id, name.clone());
            norms.insert(id, entity_norm.clone());
            if !kind.is_empty() && !entity_type.is_empty() && entity_type != kind {
                continue;
            }
            details.push((id, entity_norm, name, score));
        }
    }

    let mut lexical: Option<(i64, f32)> = None;
    let mut acronym: Option<(i64, f32)> = None;
    for (id, entity_norm, _, score) in &details {
        if lexical_match(&norm, entity_norm) {
            if lexical.is_none_or(|(_, s)| *score > s) {
                lexical = Some((*id, *score));
            }
        } else if acronym_match(&norm, entity_norm)
            && *score >= 0.6
            && acronym.is_none_or(|(_, s)| *score > s)
        {
            acronym = Some((*id, *score));
        }
    }
    if let Some((id, _)) = lexical.or(acronym) {
        let status = if lexical.is_some() {
            "lexical"
        } else {
            "acronym"
        };
        add_aliases(db, id, item, status)?;
        enrich(db, id, item, now)?;
        return Ok(Resolved {
            id,
            status: status.into(),
            similar: Vec::new(),
        });
    }

    db.sql().execute("INSERT INTO entities(name,norm,type,description,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?5)", params![item.name,norm,kind,item.description.as_deref().unwrap_or(""),now])?;
    let id = db.sql().last_insert_rowid();
    db.sql().execute(
        "INSERT OR IGNORE INTO entity_aliases(alias_norm,entity_id,origin) VALUES (?1,?2,'name')",
        params![norm, id],
    )?;
    for alias in &item.aliases {
        let alias_norm = text::norm_name(alias);
        if alias_norm.is_empty() {
            continue;
        }
        db.sql().execute(
            "INSERT OR IGNORE INTO entity_aliases(alias_norm,entity_id,origin) VALUES (?1,?2,'agent')",
            params![alias_norm, id],
        )?;
    }
    refresh_entity_fts(db, id)?;
    db.sql().execute(
        "INSERT INTO entities_vec(rowid,embedding) VALUES (?1,?2)",
        params![id, vec_blob(embedding)],
    )?;
    // Only the KNN hits create soft edges; lexical/acronym candidates are
    // handled above and never reach this branch.
    // Do not create similar soft edges when either entity norm has fewer
    // than 3 letters or contains no letters.
    let mut similar: Vec<(i64, String, f32)> = Vec::new();
    let norm_letters = norm.chars().filter(|c| c.is_alphabetic()).count();
    if norm_letters >= 3 {
        for id in &knn_order {
            let Some(name) = names.get(id) else { continue };
            let Some(other_norm) = norms.get(id) else {
                continue;
            };
            if other_norm.chars().filter(|c| c.is_alphabetic()).count() < 3 {
                continue;
            }
            let score = generated
                .iter()
                .find(|(candidate, _)| candidate == id)
                .map(|(_, score)| *score)
                .unwrap_or(0.0);
            if score >= cfg.sim_edge && !similar.iter().any(|(other, _, _)| other == id) {
                similar.push((*id, name.clone(), score));
            }
            if similar.len() == 3 {
                break;
            }
        }
    }
    Ok(Resolved {
        id,
        status: "new".into(),
        similar,
    })
}

fn resolve_same_as(db: &Db, target: &str) -> Result<i64> {
    if let Some(id) = crate::engine::parse_id(target, 'e') {
        let exists: bool = db.sql().query_row(
            "SELECT EXISTS(SELECT 1 FROM entities WHERE id=?1)",
            [id],
            |r| r.get(0),
        )?;
        if exists {
            return Ok(id);
        }
        return Err(crate::err!("unknown entity '{target}'"));
    }
    let norm = text::norm_name(target);
    db.sql()
        .query_row(
            "SELECT entity_id FROM entity_aliases WHERE alias_norm=?1 ORDER BY entity_id LIMIT 1",
            [norm],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| crate::err!("unknown entity '{target}'"))
}

/// Fill empty type/description from the mention and append new detail.
fn enrich(db: &Db, id: i64, item: &EntityIn, now: i64) -> Result<()> {
    let (mut entity_type, mut description): (String, String) = db.sql().query_row(
        "SELECT type,description FROM entities WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut changed = false;
    if entity_type.is_empty() {
        if let Some(kind) = item
            .kind
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            entity_type = kind.to_string();
            changed = true;
        }
    }
    if let Some(new) = item
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        if description.is_empty() {
            description = new.to_string();
            changed = true;
        } else if !description.contains(new) {
            let mut joined = format!("{description}; {new}");
            if joined.chars().count() > 600 {
                joined = joined.chars().take(600).collect();
            }
            description = joined;
            changed = true;
        }
    }
    if changed {
        db.sql().execute(
            "UPDATE entities SET type=?1, description=?2, updated_at=?3 WHERE id=?4",
            params![entity_type, description, now, id],
        )?;
        refresh_entity_fts(db, id)?;
        // shortcut: the entity card embedding is not refreshed on enrichment;
        // reindex/recall tolerate the stale vector until eval shows drift.
    }
    Ok(())
}

/// Add the mention's name and aliases to an entity. Never touches
/// `mention_count` (the caller counts mention rows).
fn add_aliases(db: &Db, id: i64, item: &EntityIn, origin: &str) -> Result<()> {
    let mut changed = false;
    for alias in std::iter::once(&item.name).chain(item.aliases.iter()) {
        let norm = text::norm_name(alias);
        if norm.is_empty() {
            continue;
        }
        changed |= db.sql().execute(
            "INSERT OR IGNORE INTO entity_aliases(alias_norm,entity_id,origin) VALUES (?1,?2,?3)",
            params![norm, id, origin],
        )? > 0;
    }
    if changed {
        refresh_entity_fts(db, id)?;
    }
    Ok(())
}

/// Rewrite the entity's FTS row (name, aliases, description) from the
/// relational tables.
pub(crate) fn refresh_entity_fts(db: &Db, id: i64) -> Result<()> {
    let (name, description): (String, String) = db.sql().query_row(
        "SELECT name,description FROM entities WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut st = db
        .sql()
        .prepare("SELECT alias_norm FROM entity_aliases WHERE entity_id=?1 ORDER BY alias_norm")?;
    let aliases: Vec<String> = st
        .query_map([id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    db.sql()
        .execute("DELETE FROM entities_fts WHERE rowid=?1", [id])?;
    db.sql().execute(
        "INSERT INTO entities_fts(rowid,name,aliases,description) VALUES (?1,?2,?3,?4)",
        params![id, name, aliases.join(" "), description],
    )?;
    Ok(())
}

fn lexical_match(a: &str, b: &str) -> bool {
    text::jaro_winkler(a, b) >= 0.93
        && a.chars().count() >= 5
        && b.chars().count() >= 5
        && !different_digits(a, b)
        && !different_tokens(a, b)
}

fn acronym_match(a: &str, b: &str) -> bool {
    text::acronym(a).as_deref() == Some(&b.to_uppercase())
        || text::acronym(b).as_deref() == Some(&a.to_uppercase())
}

fn different_digits(a: &str, b: &str) -> bool {
    let da: String = a.chars().filter(|c| c.is_ascii_digit()).collect();
    let db: String = b.chars().filter(|c| c.is_ascii_digit()).collect();
    !da.is_empty() && da != db
}

fn different_tokens(a: &str, b: &str) -> bool {
    let tokens = |s: &str| -> HashSet<String> {
        text::tokens(s)
            .into_iter()
            .filter(|x| !text::is_stopword(x))
            .collect()
    };
    let aa = tokens(a);
    let bb = tokens(b);
    let only_a: Vec<&String> = aa.difference(&bb).collect();
    let only_b: Vec<&String> = bb.difference(&aa).collect();
    if only_a.is_empty() && only_b.is_empty() {
        return false;
    }
    // One side is a strict subset (Paris TX vs Paris): differing tokens.
    if only_a.is_empty() || only_b.is_empty() {
        return true;
    }
    // Allow suffix/prefix extensions of the same token (postgres/postgresql);
    // block substitutions (get user / set user) and versions (v1 / v2).
    !(only_a.iter().all(|x| {
        only_b
            .iter()
            .any(|y| x.starts_with(y.as_str()) || y.starts_with(x.as_str()))
    }) && only_b.iter().all(|y| {
        only_a
            .iter()
            .any(|x| x.starts_with(y.as_str()) || y.starts_with(x.as_str()))
    }))
}

pub fn resolve_predicate(
    db: &Db,
    name: &str,
    embedding: &[f32],
    exclusive: bool,
    cfg: &Config,
) -> Result<(i64, String, bool)> {
    let norm = text::norm_predicate(name);
    if let Ok(id) = db.sql().query_row(
        "SELECT predicate_id FROM predicate_aliases WHERE alias_norm=?1",
        [&norm],
        |r| r.get(0),
    ) {
        if exclusive {
            db.sql()
                .execute("UPDATE predicates SET exclusive=1 WHERE id=?1", [id])?;
        }
        return predicate_row(db, id);
    }
    let nearest: Option<(i64, f32)> = db
        .sql()
        .query_row(
            "SELECT rowid, 1-distance FROM predicates_vec
             WHERE embedding MATCH ?1 AND k=1 ORDER BY distance",
            [vec_blob(embedding)],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f32>(1)?)),
        )
        .optional()?;
    if let Some((id, _)) = nearest.filter(|(_, s)| *s >= cfg.pred_alias) {
        db.sql().execute(
            "INSERT OR IGNORE INTO predicate_aliases(alias_norm,predicate_id) VALUES (?1,?2)",
            params![norm, id],
        )?;
        if exclusive {
            db.sql()
                .execute("UPDATE predicates SET exclusive=1 WHERE id=?1", [id])?;
        }
        return predicate_row(db, id);
    }
    db.sql().execute(
        "INSERT INTO predicates(canonical,exclusive) VALUES (?1,?2)",
        params![norm, exclusive as i64],
    )?;
    let id = db.sql().last_insert_rowid();
    db.sql().execute(
        "INSERT INTO predicate_aliases(alias_norm,predicate_id) VALUES (?1,?2)",
        params![norm, id],
    )?;
    db.sql().execute(
        "INSERT INTO predicates_vec(rowid,embedding) VALUES (?1,?2)",
        params![id, vec_blob(embedding)],
    )?;
    Ok((id, norm, exclusive))
}

fn predicate_row(db: &Db, id: i64) -> Result<(i64, String, bool)> {
    let (canonical, exclusive): (String, i64) = db.sql().query_row(
        "SELECT canonical,exclusive FROM predicates WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((id, canonical, exclusive != 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(i: usize) -> Vec<f32> {
        let mut v = vec![0.0; DIMS];
        v[i] = 1.0;
        v
    }

    fn at(cos: f32) -> Vec<f32> {
        let mut v = vec![0.0; DIMS];
        v[0] = cos;
        v[1] = (1.0 - cos * cos).sqrt();
        v
    }

    #[test]
    fn test_similar_edge_guard_requires_three_letters() {
        let db = Db::open_in_memory().unwrap();
        let cfg = Config::default();

        // 1. Entities with no letters (e.g. "7" and "11") embed close together
        let r1 = resolve_entity(
            &db,
            &EntityIn {
                name: "7".into(),
                kind: None,
                aliases: vec![],
                description: None,
                same_as: None,
            },
            &axis(0),
            100,
            &cfg,
        )
        .unwrap();
        assert_eq!(r1.similar.len(), 0);

        let r2 = resolve_entity(
            &db,
            &EntityIn {
                name: "11".into(),
                kind: None,
                aliases: vec![],
                description: None,
                same_as: None,
            },
            &at(0.95),
            100,
            &cfg,
        )
        .unwrap();
        // Should NOT create a similar edge to "7"
        assert_eq!(
            r2.similar.len(),
            0,
            "expected no similar edges between 0-letter norms, got {:?}",
            r2.similar
        );

        // 2. Entity with fewer than 3 letters (e.g. "ab") and "London"
        let r3 = resolve_entity(
            &db,
            &EntityIn {
                name: "ab".into(),
                kind: None,
                aliases: vec![],
                description: None,
                same_as: None,
            },
            &at(0.95),
            100,
            &cfg,
        )
        .unwrap();
        assert_eq!(
            r3.similar.len(),
            0,
            "expected no similar edges for < 3 letter norm 'ab', got {:?}",
            r3.similar
        );

        // 3. Normal entities with >= 3 letters create similar edges
        let r4 = resolve_entity(
            &db,
            &EntityIn {
                name: "London".into(),
                kind: None,
                aliases: vec![],
                description: None,
                same_as: None,
            },
            &axis(2),
            100,
            &cfg,
        )
        .unwrap();
        let r5 = resolve_entity(
            &db,
            &EntityIn {
                name: "Paris".into(),
                kind: None,
                aliases: vec![],
                description: None,
                same_as: None,
            },
            &{
                let mut v = vec![0.0; DIMS];
                v[2] = 0.95;
                v[3] = (1.0 - 0.95 * 0.95_f32).sqrt();
                v
            },
            100,
            &cfg,
        )
        .unwrap();
        assert_eq!(
            r5.similar.len(),
            1,
            "expected similar edge between London and Paris"
        );
        assert_eq!(r5.similar[0].0, r4.id);
    }
}
