use crate::db::Db;
use crate::embed::Mode;
use crate::engine::Engine;
use crate::engine::{
    Community, EntityCard, EntityOutput, FactHit, MemoryHit, OverviewOutput, RecallInput,
    RecallOutput,
};
use crate::{extract, graph, kg, rank, temporal, text, Result};
use rusqlite::{params, params_from_iter, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const MEMORY_COLUMNS: &str =
    "id,content,kind,tags,importance,scope,source,session,created_at,valid_from,valid_to,expired_at,superseded_by";

#[allow(clippy::chunks_exact_to_as_chunks)]
fn blob_vec(blob: Vec<u8>) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]]))
        .collect()
}

fn generation(db: &Db) -> Result<i64> {
    Ok(db.sql().query_row(
        "SELECT COALESCE((SELECT CAST(value AS INTEGER) FROM meta WHERE key='gen'),0)",
        [],
        |r| r.get(0),
    )?)
}

fn snapshot(engine: &Engine, as_of: Option<i64>, history: bool) -> Result<Arc<kg::Snapshot>> {
    let gen = generation(&engine.db)?;
    if let Some((g, a, h, snap)) = engine.snapshot_cache.lock().unwrap().as_ref() {
        if *g == gen && *a == as_of && *h == history {
            return Ok(Arc::clone(snap));
        }
    }
    let snap = Arc::new(kg::snapshot(&engine.db, as_of, history)?);
    *engine.snapshot_cache.lock().unwrap() = Some((gen, as_of, history, Arc::clone(&snap)));
    Ok(snap)
}

type MemoryRow = (
    i64,
    String,
    String,
    String,
    f64,
    String,
    String,
    String,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);

fn read_row(r: &rusqlite::Row) -> rusqlite::Result<MemoryRow> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
        r.get(12)?,
    ))
}

/// Fetch rows only for the ids a channel actually produced.
fn fetch_rows(db: &Db, ids: &[i64]) -> Result<HashMap<i64, MemoryRow>> {
    let mut out = HashMap::new();
    for chunk in ids.chunks(400) {
        let marks = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT {MEMORY_COLUMNS} FROM memories WHERE id IN ({marks})");
        let mut st = db.sql().prepare(&sql)?;
        let mut rows = st.query(params_from_iter(chunk.iter()))?;
        while let Some(r) = rows.next()? {
            let row = read_row(r)?;
            out.insert(row.0, row);
        }
    }
    Ok(out)
}

fn fetch_vectors(db: &Db, ids: &[i64]) -> Result<HashMap<i64, Vec<f32>>> {
    // vec0 serves `rowid = ?` as a point lookup but `rowid IN (...)` as a full
    // scan of the table, so fetch one row at a time.
    let mut out = HashMap::new();
    let mut st = db
        .sql()
        .prepare_cached("SELECT embedding FROM memories_vec WHERE rowid = ?1")?;
    for &id in ids {
        if let Some(blob) = st.query_row([id], |r| r.get::<_, Vec<u8>>(0)).optional()? {
            out.insert(id, blob_vec(blob));
        }
    }
    Ok(out)
}

/// World-time end of a memory (valid_to reached) or system retirement.
fn is_ended(row: &MemoryRow, cutoff: i64) -> bool {
    row.10.is_some_and(|x| x <= cutoff) || row.11.is_some()
}

fn eligible(
    row: &MemoryRow,
    as_of: Option<i64>,
    history: bool,
    input: &RecallInput,
    since: Option<i64>,
    until: Option<i64>,
) -> bool {
    let (_, _, kind, tags, _, scope, source, session, _, from, to, expired, superseded) = row;
    // Forgotten memories (expired without a successor) are never returned.
    if expired.is_some() && superseded.is_none() {
        return false;
    }
    if expired.is_some() && !history {
        return false;
    }
    if let Some(t) = as_of {
        if *from > t || to.is_some_and(|x| t >= x) {
            return false;
        }
    }
    if !input.kinds.is_empty() && !input.kinds.iter().any(|x| x == kind) {
        return false;
    }
    if !input.tags.is_empty() {
        let set: HashSet<String> = tags.split_whitespace().map(str::to_string).collect();
        if !input
            .tags
            .iter()
            .map(|x| text::fold(x))
            .any(|x| set.contains(&x))
        {
            return false;
        }
    }
    if input.scope.as_deref().is_some_and(|x| x != scope)
        || input.source.as_deref().is_some_and(|x| x != source)
        || input.session.as_deref().is_some_and(|x| x != session)
    {
        return false;
    }
    if since.is_some_and(|x| *from < x) || until.is_some_and(|x| *from >= x) {
        return false;
    }
    true
}

fn parse_bound(s: Option<&str>, clock: &temporal::Clock) -> Result<Option<i64>> {
    s.map(|x| temporal::parse_instant(x, clock).ok_or_else(|| crate::err!("invalid time '{x}'")))
        .transpose()
}

fn structured_filters(
    input: &RecallInput,
    since: Option<i64>,
    until: Option<i64>,
    as_of: Option<i64>,
) -> bool {
    !input.kinds.is_empty()
        || !input.tags.is_empty()
        || input.scope.is_some()
        || input.source.is_some()
        || input.session.is_some()
        || since.is_some()
        || until.is_some()
        || as_of.is_some()
}

/// Every entity an alias names (an alias may belong to several entities).
fn alias_entities(db: &Db, names: &[String]) -> Result<Vec<i64>> {
    let mut out = HashSet::new();
    for chunk in names.chunks(400) {
        let marks = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql =
            format!("SELECT DISTINCT entity_id FROM entity_aliases WHERE alias_norm IN ({marks})");
        let mut st = db.sql().prepare(&sql)?;
        let rows = st.query_map(params_from_iter(chunk.iter()), |r| r.get::<_, i64>(0))?;
        for row in rows {
            out.insert(row?);
        }
    }
    let mut ids: Vec<i64> = out.into_iter().collect();
    ids.sort_unstable();
    Ok(ids)
}

fn relation_visible(
    valid_from: i64,
    valid_to: Option<i64>,
    expired: Option<i64>,
    now: i64,
    history: bool,
) -> bool {
    if valid_from > now {
        return false;
    }
    if history {
        return true;
    }
    valid_to.is_none_or(|v| now < v) && expired.is_none()
}

/// Relation-fact seeds: `relations_fts` hits and `relations_vec` KNN, both
/// endpoints get half of the relation's score.
type SeedPairs = Vec<(i64, f64)>;

struct RelationSeedOptions<'a> {
    query_entities: &'a HashSet<i64>,
    fact_entity_boost: f64,
    fact_memory_seed: f64,
}

fn relation_seeds_with_config(
    db: &Db,
    query: &str,
    query_vec: &[f32],
    now: i64,
    history: bool,
    prefix_min: usize,
    options: RelationSeedOptions<'_>,
) -> Result<(SeedPairs, SeedPairs)> {
    let mut entities = Vec::new();
    let mut memories = Vec::new();
    let mut add = |relation_id: i64, src: i64, dst: i64, score: f64| -> Result<()> {
        let matched_endpoint =
            options.query_entities.contains(&src) || options.query_entities.contains(&dst);
        for id in [src, dst] {
            let weight = if options.query_entities.contains(&id) {
                score * options.fact_entity_boost
            } else {
                score
            };
            entities.push((id, weight));
        }
        if matched_endpoint && options.fact_memory_seed > 0.0 {
            let mut st = db
                .sql()
                .prepare("SELECT memory_id FROM relation_evidence WHERE relation_id=?1")?;
            for memory_id in st.query_map([relation_id], |r| r.get::<_, i64>(0))? {
                memories.push((memory_id?, score * options.fact_memory_seed));
            }
        }
        Ok(())
    };
    if let Some(q) = text::fts_query_with(query, &[], prefix_min) {
        let mut st = db.sql().prepare(
            "SELECT r.id,r.src,r.dst FROM relations_fts JOIN relations r ON r.id=relations_fts.rowid
             WHERE relations_fts MATCH ?1 AND r.valid_from<=?2
               AND (?3=1 OR ((r.valid_to IS NULL OR r.valid_to>?2) AND r.expired_at IS NULL))
             ORDER BY bm25(relations_fts,1.0,1.0) LIMIT 10",
        )?;
        let hits: Vec<(i64, i64, i64)> = st
            .query_map(params![q, now, i64::from(history)], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (rank, (relation_id, src, dst)) in hits.iter().enumerate() {
            let score = 0.5 / (1.0 + rank as f64);
            add(*relation_id, *src, *dst, score)?;
        }
    }
    {
        let mut st = db.sql().prepare(
            "SELECT rowid,1-distance FROM relations_vec
             WHERE embedding MATCH ?1 AND k=10 ORDER BY distance",
        )?;
        let hits: Vec<(i64, f32)> = st
            .query_map([crate::db::vec_blob(query_vec)], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let ids: Vec<i64> = hits
            .iter()
            .filter(|(_, cosine)| *cosine >= 0.5)
            .map(|(id, _)| *id)
            .collect();
        if !ids.is_empty() {
            let marks = std::iter::repeat_n("?", ids.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT id,src,dst,valid_from,valid_to,expired_at FROM relations WHERE id IN ({marks})"
            );
            let mut st = db.sql().prepare(&sql)?;
            let mut rows = st.query(params_from_iter(ids.iter()))?;
            let mut visible = HashMap::new();
            while let Some(r) = rows.next()? {
                let id: i64 = r.get(0)?;
                let src: i64 = r.get(1)?;
                let dst: i64 = r.get(2)?;
                let vf: i64 = r.get(3)?;
                let vt: Option<i64> = r.get(4)?;
                let expired: Option<i64> = r.get(5)?;
                if relation_visible(vf, vt, expired, now, history) {
                    visible.insert(id, (src, dst));
                }
            }
            for (id, cosine) in hits {
                if let Some(&(src, dst)) = visible.get(&id) {
                    add(id, src, dst, f64::from(cosine) * 0.5)?;
                }
            }
        }
    }
    Ok((entities, memories))
}

pub fn recall(engine: &Engine, input: &RecallInput) -> Result<RecallOutput> {
    engine.check_embedder()?;
    if input.query.trim().is_empty() {
        return Err(crate::err!("query must not be empty"));
    }
    let clock = engine.clock();
    let now = clock.now;
    let as_of = parse_bound(input.as_of.as_deref(), &clock)?;
    let since = parse_bound(input.since.as_deref(), &clock)?;
    let until = parse_bound(input.until.as_deref(), &clock)?;
    let window = temporal::find_window(&input.query, &clock);
    let history =
        input.include_history || temporal::history_intent(&input.query) || as_of.is_some();
    let k = input.k.unwrap_or(8).clamp(1, 50);
    let budget = input.budget.unwrap_or(1200);
    let query_vec = engine.emb.embed(&[&input.query], Mode::Query)?.remove(0);
    let depth = (5 * k).max(50);
    // Filtered channels need headroom: the KNN/BM25 depth is applied before
    // eligibility, so oversample when structured filters are present.
    let fetch_depth = if structured_filters(input, since, until, as_of) {
        (depth * 4).min(2000)
    } else {
        depth
    };
    let mut rows: HashMap<i64, MemoryRow> = HashMap::new();

    // ------------------------------------------------------------ bm25
    let mut bm25_scored: Vec<(i64, f64)> = Vec::new();
    if let Some(q) = text::fts_query_with(&input.query, &input.keywords, engine.cfg.fts_prefix_min)
    {
        let mut st = engine.db.sql().prepare(
            "SELECT rowid,bm25(memories_fts,1.0,2.0) FROM memories_fts
             WHERE memories_fts MATCH ?1 ORDER BY bm25(memories_fts,1.0,2.0) LIMIT ?2",
        )?;
        for row in st.query_map(params![q, fetch_depth as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
        })? {
            bm25_scored.push(row?);
        }
    }
    let missing: Vec<i64> = bm25_scored
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !rows.contains_key(id))
        .collect();
    rows.extend(fetch_rows(&engine.db, &missing)?);
    let bm25_scored: Vec<(i64, f64)> = bm25_scored
        .into_iter()
        .filter(|(id, _)| {
            rows.get(id)
                .is_some_and(|row| eligible(row, as_of, history, input, since, until))
        })
        .collect();
    let best_bm25 = bm25_scored
        .iter()
        .map(|(_, score)| score.abs())
        .fold(0.0f64, f64::max);
    let bm25: Vec<i64> = bm25_scored
        .into_iter()
        .filter(|(_, score)| score.abs() >= engine.cfg.bm25_rel * best_bm25)
        .map(|(id, _)| id)
        .take(depth)
        .collect();

    // ---------------------------------------------------------- vector
    let knn_sql = if history {
        "SELECT rowid,1-distance FROM memories_vec
         WHERE embedding MATCH ?1 AND k=?2 ORDER BY distance"
    } else {
        "SELECT rowid,1-distance FROM memories_vec
         WHERE embedding MATCH ?1 AND k=?2 AND live=1 ORDER BY distance"
    };
    let mut st = engine.db.sql().prepare(knn_sql)?;
    let knn: Vec<(i64, f32)> = st
        .query_map(
            params![crate::db::vec_blob(&query_vec), fetch_depth as i64],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f32>(1)?)),
        )?
        .collect::<rusqlite::Result<_>>()?;
    let missing: Vec<i64> = knn
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !rows.contains_key(id))
        .collect();
    rows.extend(fetch_rows(&engine.db, &missing)?);
    let mut vec_ranked: Vec<(i64, f64)> = knn
        .into_iter()
        .filter(|(id, _)| {
            rows.get(id)
                .is_some_and(|row| eligible(row, as_of, history, input, since, until))
        })
        .map(|(id, cosine)| (id, f64::from(cosine)))
        .collect();
    vec_ranked.truncate(depth);
    let vec_ids: Vec<i64> = vec_ranked.iter().map(|(id, _)| *id).collect();

    // ------------------------------------------------------- temporal
    let mut time_ids: Vec<i64> = Vec::new();
    if let Some(w) = &window {
        let mut st = engine.db.sql().prepare(
            // Happened/started inside the window (an older fact that is still
            // open is not "in" last week). Newest 400, plus the in-window
            // members of the vector list so wide windows keep relevance.
            "SELECT id FROM memories WHERE valid_from >= ?1 AND valid_from < ?2
             ORDER BY valid_from DESC LIMIT 400",
        )?;
        let mut candidates: Vec<i64> = st
            .query_map(params![w.start, w.end], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<_>>()?;
        let seen: HashSet<i64> = candidates.iter().copied().collect();
        candidates.extend(vec_ids.iter().copied().filter(|id| {
            !seen.contains(id)
                && rows
                    .get(id)
                    .is_some_and(|row| row.9 >= w.start && row.9 < w.end)
        }));
        let missing: Vec<i64> = candidates
            .iter()
            .copied()
            .filter(|id| !rows.contains_key(id))
            .collect();
        rows.extend(fetch_rows(&engine.db, &missing)?);
        let in_window: Vec<i64> = candidates
            .into_iter()
            .filter(|id| {
                rows.get(id)
                    .is_some_and(|row| eligible(row, as_of, history, input, since, until))
            })
            .collect();
        let vectors = fetch_vectors(&engine.db, &in_window)?;
        let mut scored: Vec<(i64, f64)> = in_window
            .into_iter()
            .filter_map(|id| {
                vectors
                    .get(&id)
                    .map(|v| (id, f64::from(rank::cosine(&query_vec, v))))
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(depth);
        time_ids = scored.into_iter().map(|(id, _)| id).collect();
    }

    // ---------------------------------------------------------- graph
    let snap = snapshot(engine, as_of, history)?;
    let mut ent_seeds: HashMap<i64, f64> = HashMap::new();
    let mut seed_entity = |id: i64, weight: f64| {
        let entry = ent_seeds.entry(id).or_insert(0.0);
        if weight > *entry {
            *entry = weight;
        }
    };
    // (a) exact alias hits in the query and in agent-supplied entities.
    let mut names: Vec<String> = extract::ngrams(&input.query, 4)
        .into_iter()
        .map(|x| x.0)
        .collect();
    names.extend(input.entities.iter().map(|x| text::norm_name(x)));
    names.sort();
    names.dedup();
    let query_alias_entities: HashSet<i64> =
        alias_entities(&engine.db, &names)?.into_iter().collect();
    for id in &query_alias_entities {
        seed_entity(*id, 1.0);
    }
    // (b) entity KNN against the query vector.
    {
        let mut st = engine.db.sql().prepare(
            "SELECT rowid,1-distance FROM entities_vec
             WHERE embedding MATCH ?1 AND k=10 ORDER BY distance",
        )?;
        let hits: Vec<(i64, f32)> = st
            .query_map([crate::db::vec_blob(&query_vec)], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        for (id, cosine) in hits {
            if cosine >= engine.cfg.entity_seed {
                seed_entity(id, f64::from(cosine));
            }
        }
    }
    // (c) relation-fact hits.
    let (relation_entities, relation_memories) = relation_seeds_with_config(
        &engine.db,
        &input.query,
        &query_vec,
        now,
        history,
        engine.cfg.fts_prefix_min,
        RelationSeedOptions {
            query_entities: &query_alias_entities,
            fact_entity_boost: engine.cfg.fact_entity_boost,
            fact_memory_seed: engine.cfg.fact_memory_seed,
        },
    )?;
    for (id, weight) in relation_entities {
        seed_entity(id, weight);
    }
    // Specificity: one GROUP BY, not one query per seed.
    let seed_ids: Vec<i64> = ent_seeds.keys().copied().collect();
    let mut specificity: HashMap<i64, i64> = HashMap::new();
    for chunk in seed_ids.chunks(400) {
        let marks = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql =
            format!("SELECT entity_id,COUNT(*) FROM mentions WHERE entity_id IN ({marks}) GROUP BY entity_id");
        let mut st = engine.db.sql().prepare(&sql)?;
        let mut rows = st.query(params_from_iter(chunk.iter()))?;
        while let Some(r) = rows.next()? {
            specificity.insert(r.get(0)?, r.get(1)?);
        }
    }
    for (id, weight) in ent_seeds.iter_mut() {
        let mentions = specificity.get(id).copied().unwrap_or(0);
        *weight /= mentions.max(1) as f64;
    }
    let mut mem_seed_weights: HashMap<i64, f64> = vec_ranked
        .iter()
        .take(engine.cfg.mem_seed_top)
        .map(|(id, cosine)| (*id, cosine.max(0.0)))
        .collect();
    for (id, weight) in relation_memories {
        mem_seed_weights
            .entry(id)
            .and_modify(|current| *current = current.max(weight))
            .or_insert(weight);
    }
    let mem_seeds: Vec<(i64, f64)> = mem_seed_weights.into_iter().collect();
    let ent_total: f64 = ent_seeds.values().sum();
    let mem_total: f64 = mem_seeds.iter().map(|(_, w)| w).sum();
    let mut seeds: Vec<(u32, f64)> = Vec::new();
    if ent_total > 0.0 {
        for (id, weight) in &ent_seeds {
            if let Some(&node) = snap.ent_node.get(id) {
                seeds.push((node, weight * (1.0 - engine.cfg.mem_seed_share) / ent_total));
            }
        }
    }
    if mem_total > 0.0 {
        let target = if ent_total > 0.0 {
            engine.cfg.mem_seed_share
        } else {
            1.0
        };
        for (id, weight) in &mem_seeds {
            if let Some(&node) = snap.mem_node.get(id) {
                seeds.push((node, weight * target / mem_total));
            }
        }
    }
    let ppr = graph::ppr_push(&snap.csr, &seeds, engine.cfg.alpha, engine.cfg.eps, 200_000);
    let mut graph_ids: Vec<i64> = Vec::new();
    let mut ppr_score: HashMap<i64, f64> = HashMap::new();
    for (node, score) in ppr {
        match snap.node_kind.get(node as usize) {
            Some(('m', id)) => graph_ids.push(*id),
            Some(('e', id)) => {
                ppr_score.insert(*id, score);
            }
            _ => {}
        }
    }
    graph_ids.truncate(fetch_depth);
    let missing: Vec<i64> = graph_ids
        .iter()
        .copied()
        .filter(|id| !rows.contains_key(id))
        .collect();
    rows.extend(fetch_rows(&engine.db, &missing)?);
    let graph_ids: Vec<i64> = graph_ids
        .into_iter()
        .filter(|id| {
            rows.get(id)
                .is_some_and(|row| eligible(row, as_of, history, input, since, until))
        })
        .take(depth)
        .collect();

    let channels = [
        rank::Channel {
            name: "bm25",
            weight: engine.cfg.w_bm25,
            ids: &bm25,
        },
        rank::Channel {
            name: "vec",
            weight: engine.cfg.w_vector,
            ids: &vec_ids,
        },
        rank::Channel {
            name: "graph",
            weight: engine.cfg.w_graph,
            ids: &graph_ids,
        },
        rank::Channel {
            name: "time",
            weight: engine.cfg.w_temporal,
            ids: &time_ids,
        },
    ];
    let fused = rank::rrf(&channels, engine.cfg.rrf_k);
    let cutoff = as_of.unwrap_or(now);
    let mut candidates = Vec::new();
    for f in fused {
        let Some(row) = rows.get(&f.id) else { continue };
        let mut score = f.score;
        if is_ended(row, cutoff) {
            if !history {
                score *= engine.cfg.expired_penalty;
            } else if as_of.is_none() {
                // "where did X live before": the past state is the answer.
                score *= engine.cfg.history_boost;
            }
        }
        if window
            .as_ref()
            .is_some_and(|w| row.9 >= w.start && row.9 < w.end)
        {
            score *= 1.25;
        }
        let age = (now - row.9).max(0) as f64 / 86_400.0;
        score *= 0.9 + 0.1 * 2f64.powf(-age / engine.cfg.half_life_days);
        score *= 0.9 + 0.2 * row.4;
        candidates.push((f.id, score, f.ranks));
    }
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mmr_ids: Vec<i64> = candidates.iter().map(|x| x.0).take(3 * k).collect();
    let vecs = fetch_vectors(&engine.db, &mmr_ids)?;
    let mmr_input: Vec<(i64, f64)> = candidates.iter().map(|x| (x.0, x.1)).take(3 * k).collect();
    let selected = rank::mmr(&mmr_input, &vecs, engine.cfg.mmr_lambda, k);
    let costs: Vec<usize> = selected
        .iter()
        .map(|id| {
            rows.get(id)
                .map(|r| (r.1.len() + 12).div_ceil(4))
                .unwrap_or(0)
        })
        .collect();
    let kept = rank::pack(&costs, budget);
    let kept_ids: HashSet<i64> = kept.iter().map(|&i| selected[i]).collect();
    let mut memories = Vec::new();
    for id in &selected {
        let Some(row) = rows.get(id) else { continue };
        let ranks = candidates
            .iter()
            .find(|x| x.0 == *id)
            .map(|x| x.2.clone())
            .unwrap_or_default();
        let mut reasons: Vec<String> = ranks.into_iter().map(|(n, r)| format!("{n}#{r}")).collect();
        if window
            .as_ref()
            .is_some_and(|w| row.9 >= w.start && row.9 < w.end)
        {
            reasons.push("time".into());
        }
        if kept_ids.contains(id) {
            memories.push(MemoryHit {
                id: *id,
                kind: row.2.clone(),
                content: row.1.clone(),
                valid_from: row.9,
                valid_to: row.10,
                ended: is_ended(row, cutoff),
                superseded_by: row.12,
                tags: row.3.split_whitespace().map(str::to_string).collect(),
                score: candidates
                    .iter()
                    .find(|x| x.0 == *id)
                    .map(|x| x.1)
                    .unwrap_or(0.0),
                reasons,
            });
        }
    }
    let tokens_used: usize = kept.iter().map(|&i| costs[i]).sum();
    if !selected.is_empty() {
        let marks = std::iter::repeat_n("?", selected.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "UPDATE memories SET access_count=access_count+1,last_access=?1 WHERE id IN ({marks})"
        );
        let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];
        p.extend(
            selected
                .iter()
                .map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>),
        );
        engine.db.sql().execute(
            &sql,
            rusqlite::params_from_iter(p.iter().map(|x| x.as_ref())),
        )?;
    }
    let mut relevant_entities: HashSet<i64> = ent_seeds.keys().copied().collect();
    let returned_memory_ids: Vec<i64> = memories.iter().map(|memory| memory.id).collect();
    for chunk in returned_memory_ids.chunks(400) {
        if chunk.is_empty() {
            continue;
        }
        let marks = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT DISTINCT entity_id FROM mentions WHERE memory_id IN ({marks})");
        let mut st = engine.db.sql().prepare(&sql)?;
        for entity_id in st.query_map(params_from_iter(chunk.iter()), |r| r.get::<_, i64>(0))? {
            relevant_entities.insert(entity_id?);
        }
    }
    let top_entities: Vec<i64> = ppr_score
        .iter()
        .filter(|(id, _)| relevant_entities.contains(id) && snap.ent_node.contains_key(id))
        .map(|(id, s)| (*id, *s))
        .sorted_by_score()
        .into_iter()
        .take(10)
        .map(|x| x.0)
        .collect();
    let facts = facts_for(engine, &top_entities, history, cutoff, 8)?;
    let entities = top_entities
        .iter()
        .take(3)
        .filter_map(|id| {
            let keep = engine
                .db
                .sql()
                .query_row(
                    "SELECT type,description FROM entities WHERE id=?1",
                    [id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .ok()
                .is_some_and(|(kind, description)| {
                    relevant_entities.contains(id) && (!kind.is_empty() || !description.is_empty())
                });
            if keep || ent_seeds.contains_key(id) {
                Some(card(engine, *id))
            } else {
                None
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(RecallOutput {
        memories,
        facts,
        entities,
        window: window.map(|w| (w.start, w.end, w.matched)),
        candidates: candidates.len(),
        tokens_used,
        tokens_budget: budget,
        omitted: selected.len().saturating_sub(kept.len()),
    })
}

trait ScoreOrder {
    fn sorted_by_score(self) -> Vec<(i64, f64)>;
}
impl<I: Iterator<Item = (i64, f64)>> ScoreOrder for I {
    fn sorted_by_score(self) -> Vec<(i64, f64)> {
        let mut v: Vec<_> = self.collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        v
    }
}

fn card(engine: &Engine, id: i64) -> Result<EntityCard> {
    let (name, kind, desc): (String, String, String) = engine.db.sql().query_row(
        "SELECT name,type,description FROM entities WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let norm = text::norm_name(&name);
    let mut st = engine
        .db
        .sql()
        .prepare("SELECT alias_norm FROM entity_aliases WHERE entity_id=?1 AND alias_norm!=?2")?;
    let aliases = st
        .query_map(params![id, norm], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(EntityCard {
        id,
        name,
        kind,
        aliases,
        description: desc,
    })
}

fn facts_for(
    engine: &Engine,
    ids: &[i64],
    history: bool,
    cutoff: i64,
    limit: usize,
) -> Result<Vec<FactHit>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let set: HashSet<i64> = ids.iter().copied().collect();
    let mut st = engine.db.sql().prepare("SELECT r.id,r.src,r.dst,p.canonical,r.fact,r.valid_from,r.valid_to,r.expired_at,e.name,d.name FROM relations r JOIN predicates p ON p.id=r.predicate_id JOIN entities e ON e.id=r.src JOIN entities d ON d.id=r.dst")?;
    let mut out = Vec::new();
    for x in st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
        ))
    })? {
        let (id, src, dst, pred, fact, vf, vt, expired, source, target) = x?;
        if (!set.contains(&src) && !set.contains(&dst)) || vf > cutoff {
            continue;
        }
        if !history && (expired.is_some() || vt.is_some_and(|v| cutoff >= v)) {
            continue;
        }
        out.push(FactHit {
            id,
            source,
            predicate: pred,
            target,
            fact,
            valid_from: vf,
            valid_to: vt,
            ended: expired.is_some() || vt.is_some_and(|v| v <= cutoff),
        });
    }
    out.truncate(limit);
    Ok(out)
}

pub fn entity(engine: &Engine, name: &str, limit: usize) -> Result<EntityOutput> {
    let id = crate::engine::parse_id(name, 'e')
        .or_else(|| {
            let n = text::norm_name(name);
            engine
                .db
                .sql()
                .query_row(
                    "SELECT entity_id FROM entity_aliases WHERE alias_norm=?1 LIMIT 1",
                    [&n],
                    |r| r.get(0),
                )
                .ok()
        })
        .map(Ok)
        .unwrap_or_else(|| lookup_entity_fuzzy(engine, name))?;
    let now = engine.clock().now;
    let card = card(engine, id)?;
    let mut current = facts_for(engine, &[id], false, now, limit)?;
    let all = facts_for(engine, &[id], true, now, usize::MAX)?;
    let past: Vec<_> = all
        .into_iter()
        .filter(|f| !current.iter().any(|x| x.id == f.id))
        .collect();
    let mut st = engine.db.sql().prepare(
        "SELECT CASE WHEN a=?1 THEN b ELSE a END, score FROM similar WHERE a=?1 OR b=?1",
    )?;
    let similar = st
        .query_map([id], |r| {
            let other: i64 = r.get(0)?;
            let score: f32 = r.get(1)?;
            let name: String = engine.db.sql().query_row(
                "SELECT name FROM entities WHERE id=?1",
                [other],
                |x| x.get(0),
            )?;
            Ok((other, name, score))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut ts = engine.db.sql().prepare("SELECT id,kind,content,valid_from,valid_to,expired_at,superseded_by,tags,importance FROM memories WHERE id IN (SELECT memory_id FROM mentions WHERE entity_id=?1) AND NOT (expired_at IS NOT NULL AND superseded_by IS NULL) ORDER BY valid_from DESC LIMIT ?2")?;
    let timeline = ts
        .query_map(params![id, limit.max(1) as i64], |r| {
            let valid_to: Option<i64> = r.get(4)?;
            let expired: Option<i64> = r.get(5)?;
            Ok(MemoryHit {
                id: r.get(0)?,
                kind: r.get(1)?,
                content: r.get(2)?,
                valid_from: r.get(3)?,
                valid_to,
                ended: expired.is_some() || valid_to.is_some_and(|x| x <= now),
                superseded_by: r.get(6)?,
                tags: r
                    .get::<_, String>(7)?
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
                score: 0.0,
                reasons: vec![],
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    current.truncate(limit);
    Ok(EntityOutput {
        card,
        current_facts: current,
        past_facts: past.into_iter().take(limit).collect(),
        similar,
        timeline,
    })
}

/// Fallbacks after an exact alias miss: the unique-best alias prefix
/// ("UltraBERT" -> "UltraBERT-Embed-150M"), then the nearest entity card at
/// cosine >= 0.8; otherwise an error that names the closest entities.
fn lookup_entity_fuzzy(engine: &Engine, name: &str) -> Result<i64> {
    let n = text::norm_name(name);
    if n.chars().count() >= 3 {
        let prefix: Option<i64> = engine
            .db
            .sql()
            .query_row(
                "SELECT a.entity_id FROM entity_aliases a JOIN entities e ON e.id = a.entity_id
                 WHERE a.alias_norm >= ?1 AND a.alias_norm < ?1 || char(1114111)
                 ORDER BY e.mention_count DESC, length(a.alias_norm) LIMIT 1",
                [&n],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = prefix {
            return Ok(id);
        }
    }
    let vector = engine.emb.embed(&[name], Mode::Document)?.remove(0);
    let mut st = engine.db.sql().prepare(
        "SELECT v.rowid, 1 - v.distance, e.name FROM entities_vec v JOIN entities e ON e.id = v.rowid
         WHERE v.embedding MATCH ?1 AND k = 3 ORDER BY v.distance",
    )?;
    let near: Vec<(i64, f64, String)> = st
        .query_map([crate::db::vec_blob(&vector)], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    match near.first() {
        Some((id, cos, _)) if *cos >= 0.8 => Ok(*id),
        _ => Err(crate::err!(
            "unknown entity '{name}'{}",
            if near.is_empty() {
                String::new()
            } else {
                format!(
                    "; closest: {}",
                    near.iter()
                        .map(|(id, _, n)| format!("{n} (e{id})"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        )),
    }
}

fn overview_ppr(engine: &Engine, query: &str) -> Result<HashMap<i64, f64>> {
    let vector = engine.emb.embed(&[query], Mode::Query)?.remove(0);
    let snap = snapshot(engine, None, false)?;
    let mut seeds = Vec::new();
    let names = extract::ngrams(query, 4)
        .into_iter()
        .map(|x| x.0)
        .collect::<Vec<_>>();
    for id in alias_entities(&engine.db, &names)? {
        if let Some(&node) = snap.ent_node.get(&id) {
            seeds.push((node, 1.0));
        }
    }
    let mut st = engine.db.sql().prepare(
        "SELECT rowid,1-distance FROM entities_vec
         WHERE embedding MATCH ?1 AND k=10 ORDER BY distance",
    )?;
    for row in st.query_map([crate::db::vec_blob(&vector)], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
    })? {
        let (id, score) = row?;
        if score >= f64::from(engine.cfg.entity_seed) {
            if let Some(&node) = snap.ent_node.get(&id) {
                seeds.push((node, score));
            }
        }
    }
    // Topical queries ("embedding performance") rarely name an entity: seed the
    // walk from the nearest memories too, with the same share recall uses.
    let ent_total: f64 = seeds.iter().map(|(_, w)| w).sum();
    let mut st = engine.db.sql().prepare(
        "SELECT rowid,1-distance FROM memories_vec
         WHERE embedding MATCH ?1 AND k=?2 AND live=1 ORDER BY distance",
    )?;
    let mem: Vec<(i64, f64)> = st
        .query_map(
            params![crate::db::vec_blob(&vector), engine.cfg.mem_seed_top as i64],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<rusqlite::Result<_>>()?;
    let mem_total: f64 = mem.iter().map(|(_, w)| w.max(0.0)).sum();
    if mem_total > 0.0 {
        let share = if ent_total > 0.0 {
            engine.cfg.mem_seed_share
        } else {
            1.0
        };
        for (_, w) in seeds.iter_mut() {
            *w *= (1.0 - share) / ent_total;
        }
        for (id, w) in mem {
            if let Some(&node) = snap.mem_node.get(&id) {
                seeds.push((node, w.max(0.0) * share / mem_total));
            }
        }
    }
    Ok(
        graph::ppr_push(&snap.csr, &seeds, engine.cfg.alpha, engine.cfg.eps, 200_000)
            .into_iter()
            .filter_map(|(node, score)| match snap.node_kind.get(node as usize) {
                Some(('e', id)) => Some((*id, score)),
                _ => None,
            })
            .collect(),
    )
}

fn representative(engine: &Engine, ids: &[i64]) -> Result<Option<MemoryHit>> {
    if ids.is_empty() {
        return Ok(None);
    }
    let marks = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT m.id,m.kind,m.content,m.valid_from,m.valid_to,m.expired_at,m.superseded_by,m.tags
         FROM memories m JOIN mentions mn ON mn.memory_id=m.id
         WHERE mn.entity_id IN ({marks}) AND m.expired_at IS NULL
         GROUP BY m.id
         ORDER BY COUNT(DISTINCT mn.entity_id) DESC,m.importance DESC,m.valid_from DESC
         LIMIT 1"
    );
    let row = engine
        .db
        .sql()
        .query_row(&sql, params_from_iter(ids.iter()), |r| {
            let valid_to: Option<i64> = r.get(4)?;
            let expired: Option<i64> = r.get(5)?;
            Ok(MemoryHit {
                id: r.get(0)?,
                kind: r.get(1)?,
                content: r.get(2)?,
                valid_from: r.get(3)?,
                valid_to,
                ended: expired.is_some() || valid_to.is_some_and(|x| x <= engine.clock().now),
                superseded_by: r.get(6)?,
                tags: r
                    .get::<_, String>(7)?
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
                score: 0.0,
                reasons: Vec::new(),
            })
        })
        .optional()?;
    Ok(row)
}

pub fn overview(engine: &Engine, query: Option<&str>, k: usize) -> Result<OverviewOutput> {
    let comm = kg::communities(&engine.db)?;
    let mut groups: HashMap<i64, Vec<i64>> = HashMap::new();
    for (e, c) in comm {
        groups.entry(c).or_default().push(e);
    }
    let ppr = query.map(|q| overview_ppr(engine, q)).transpose()?;
    let mut out = Vec::new();
    for (cid, ids) in groups {
        let mut ranked = ids
            .iter()
            .map(|id| {
                let (kind, description, mentions): (String, String, i64) =
                    engine.db.sql().query_row(
                        "SELECT type,description,mention_count FROM entities WHERE id=?1",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )?;
                Ok((
                    *id,
                    (!kind.is_empty() || !description.is_empty()),
                    mentions,
                    ppr.as_ref().and_then(|x| x.get(id)).copied().unwrap_or(0.0),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        if ranked
            .iter()
            .all(|(_, typed, mentions, _)| !*typed && *mentions <= 1)
        {
            continue;
        }
        ranked.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| b.3.partial_cmp(&a.3).unwrap())
        });
        let ordered_ids: Vec<i64> = ranked.iter().map(|x| x.0).collect();
        let entities = ordered_ids
            .iter()
            .take(3)
            .map(|id| card(engine, *id))
            .collect::<Result<Vec<_>>>()?;
        let facts = facts_for(engine, &ordered_ids, false, engine.clock().now, 3)?;
        let score = query
            .map(|_| ranked.iter().map(|x| x.3).sum())
            .unwrap_or_else(|| ranked.iter().map(|x| x.2 as f64).sum());
        out.push(Community {
            id: cid,
            size: ordered_ids.len(),
            entities,
            facts,
            representative: representative(engine, &ordered_ids)?,
            score,
        });
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    out.truncate(k.max(1));
    Ok(OverviewOutput { communities: out })
}
