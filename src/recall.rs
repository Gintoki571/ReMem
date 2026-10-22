use crate::db::Db;
use crate::embed::Mode;
use crate::engine::Engine;
use crate::engine::{
    Community, EntityCard, EntityOutput, FactHit, MemoryHit, OverviewOutput, RecallInput,
    RecallOutput,
};
use crate::{extract, graph, kg, rank, temporal, text, Result};
use rusqlite::params;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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

fn eligible(
    row: &MemoryRow,
    _now: i64,
    as_of: Option<i64>,
    history: bool,
    input: &RecallInput,
) -> bool {
    let (_, _, kind, tags, _, scope, source, session, _, from, to, expired, superseded) = row;
    if expired.is_some() && superseded.is_none() {
        return false;
    }
    if expired.is_some() && superseded.is_some() && !history {
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
    true
}

fn parse_bound(s: Option<&str>, clock: &temporal::Clock) -> Result<Option<i64>> {
    s.map(|x| temporal::parse_instant(x, clock).ok_or_else(|| crate::err!("invalid time '{x}'")))
        .transpose()
}

pub fn recall(engine: &Engine, input: &RecallInput) -> Result<RecallOutput> {
    engine.check_embedder()?;
    if input.query.trim().is_empty() {
        return Err(crate::err!("query must not be empty"));
    }
    let clock = engine.clock();
    let as_of = parse_bound(input.as_of.as_deref(), &clock)?;
    let since = parse_bound(input.since.as_deref(), &clock)?;
    let until = parse_bound(input.until.as_deref(), &clock)?;
    let window = temporal::find_window(&input.query, &clock);
    let history =
        input.include_history || temporal::history_intent(&input.query) || as_of.is_some();
    let k = input.k.unwrap_or(8).clamp(1, 50);
    let budget = input.budget.unwrap_or(1200);
    let query_vec = engine.emb.embed(&[&input.query], Mode::Query)?.remove(0);
    let extra = input.keywords.clone();
    let fts = text::fts_query(&input.query, &extra);
    let mut rows = Vec::new();
    let mut st = engine.db.sql().prepare("SELECT id,content,kind,tags,importance,scope,source,session,created_at,valid_from,valid_to,expired_at,superseded_by FROM memories")?;
    for r in st.query_map([], |r| {
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
    })? {
        let row = r?;
        if !eligible(&row, clock.now, as_of, history, input)
            || since.is_some_and(|x| row.9 < x)
            || until.is_some_and(|x| row.9 >= x)
        {
            continue;
        }
        rows.push(row);
    }
    let allowed: HashSet<i64> = rows.iter().map(|r| r.0).collect();
    let mut bm25 = Vec::new();
    if let Some(q) = fts {
        let mut s = engine.db.sql().prepare(
            "SELECT rowid FROM memories_fts WHERE memories_fts MATCH ?1 ORDER BY bm25(memories_fts,1.0,2.0) LIMIT ?2",
        )?;
        for x in s.query_map(params![q, (5 * k).max(50) as i64], |r| r.get::<_, i64>(0))? {
            let id = x?;
            if allowed.contains(&id) {
                bm25.push(id);
            }
        }
    }
    let mut vec_scores = HashMap::new();
    let mut vec_ranked = Vec::new();
    let mut vs = engine
        .db
        .sql()
        .prepare("SELECT rowid,embedding FROM memories_vec")?;
    for x in vs.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))? {
        let (id, blob) = x?;
        if allowed.contains(&id) {
            let score = rank::cosine(&query_vec, &blob_vec(blob)) as f64;
            vec_scores.insert(id, score);
            vec_ranked.push((id, score));
        }
    }
    vec_ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    vec_ranked.truncate((5 * k).max(50));
    let vec_ids: Vec<i64> = vec_ranked.iter().map(|x| x.0).collect();

    let snap = snapshot(engine, as_of, history)?;
    let mut seeds = Vec::new();
    let names: Vec<String> = extract::ngrams(&input.query, 4)
        .into_iter()
        .map(|x| x.0)
        .chain(input.entities.iter().map(|x| text::norm_name(x)))
        .collect();
    for name in names {
        if let Ok(id) = engine.db.sql().query_row(
            "SELECT entity_id FROM entity_aliases WHERE alias_norm=?1 LIMIT 1",
            [&name],
            |r| r.get::<_, i64>(0),
        ) {
            if let Some(&node) = snap.ent_node.get(&id) {
                let mentions: i64 = engine.db.sql().query_row(
                    "SELECT COUNT(*) FROM mentions WHERE entity_id=?1",
                    [id],
                    |r| r.get(0),
                )?;
                seeds.push((node, 1.0 / (mentions.max(1) as f64)));
            }
        }
    }
    for &(id, score) in vec_ranked.iter().take(20) {
        if let Some(&node) = snap.mem_node.get(&id) {
            seeds.push((node, score.max(0.0) * 0.05));
        }
    }
    let ppr = graph::ppr_push(&snap.csr, &seeds, engine.cfg.alpha, engine.cfg.eps, 200_000);
    let mut graph_ids = Vec::new();
    let mut ppr_score = HashMap::new();
    for (node, score) in ppr {
        if let Some(('m', id)) = snap.node_kind.get(node as usize) {
            if allowed.contains(id) {
                graph_ids.push(*id);
            }
        }
        if let Some((_, id)) = snap.node_kind.get(node as usize) {
            ppr_score.insert(*id, score);
        }
    }
    let mut time_ids: Vec<i64> = Vec::new();
    if let Some(w) = &window {
        let mut ranked_time: Vec<(i64, f64)> = rows
            .iter()
            .filter(|r| r.9 >= w.start && r.9 < w.end)
            .filter_map(|r| vec_scores.get(&r.0).map(|s| (r.0, *s)))
            .collect();
        ranked_time.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        time_ids = ranked_time.into_iter().map(|x| x.0).collect();
    }
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
    let mut candidates = Vec::new();
    for f in fused {
        if let Some(row) = rows.iter().find(|r| r.0 == f.id) {
            let mut score = f.score;
            if row.11.is_some() && !history {
                score *= engine.cfg.expired_penalty;
            }
            if window
                .as_ref()
                .is_some_and(|w| row.9 >= w.start && row.9 < w.end)
            {
                score *= 1.25;
            }
            let age = (clock.now - row.9).max(0) as f64 / 86_400.0;
            score *= 0.9 + 0.1 * 2f64.powf(-age / engine.cfg.half_life_days);
            score *= 0.9 + 0.2 * row.4;
            candidates.push((f.id, score, f.ranks));
        }
    }
    let mut vecs = HashMap::new();
    for &(id, _, _) in &candidates {
        if let Ok(blob) = engine.db.sql().query_row(
            "SELECT embedding FROM memories_vec WHERE rowid=?1",
            [id],
            |r| r.get::<_, Vec<u8>>(0),
        ) {
            vecs.insert(id, blob_vec(blob));
        }
    }
    let mmr_input: Vec<(i64, f64)> = candidates.iter().map(|x| (x.0, x.1)).take(3 * k).collect();
    let selected = rank::mmr(&mmr_input, &vecs, engine.cfg.mmr_lambda, k);
    let costs: Vec<usize> = selected
        .iter()
        .map(|id| {
            rows.iter()
                .find(|r| r.0 == *id)
                .map(|r| (r.1.len() + 12).div_ceil(4))
                .unwrap_or(0)
        })
        .collect();
    let kept = rank::pack(&costs, budget);
    let kept_ids: HashSet<i64> = kept.iter().map(|&i| selected[i]).collect();
    let mut memories = Vec::new();
    for id in &selected {
        let row = rows.iter().find(|r| r.0 == *id).unwrap();
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
                ended: row.10.is_some_and(|x| x <= clock.now) || row.11.is_some(),
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
        let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(clock.now)];
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
    let top_entities: Vec<i64> = ppr_score
        .iter()
        .filter_map(|(id, s)| {
            if snap.ent_node.contains_key(id) {
                Some((*id, *s))
            } else {
                None
            }
        })
        .sorted_by_score()
        .into_iter()
        .take(10)
        .map(|x| x.0)
        .collect();
    let facts = facts_for(
        engine,
        &top_entities,
        history,
        as_of.unwrap_or(clock.now),
        8,
    )?;
    let entities = top_entities
        .iter()
        .take(3)
        .map(|id| card(engine, *id))
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
    now: i64,
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
        if (!set.contains(&src) && !set.contains(&dst))
            || (!history && expired.is_some())
            || vf > now
            || vt.is_some_and(|v| now >= v) && !history
        {
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
            ended: expired.is_some() || vt.is_some_and(|v| v <= now),
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
        .ok_or_else(|| crate::err!("unknown entity '{name}'"))?;
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
            Ok(MemoryHit {
                id: r.get(0)?,
                kind: r.get(1)?,
                content: r.get(2)?,
                valid_from: r.get(3)?,
                valid_to: r.get(4)?,
                ended: r.get::<_, Option<i64>>(5)?.is_some()
                    || r.get::<_, Option<i64>>(4)?.is_some_and(|x| x <= now),
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

pub fn overview(engine: &Engine, query: Option<&str>, k: usize) -> Result<OverviewOutput> {
    let comm = kg::communities(&engine.db)?;
    let mut groups: HashMap<i64, Vec<i64>> = HashMap::new();
    for (e, c) in comm {
        groups.entry(c).or_default().push(e);
    }
    let mut out = Vec::new();
    for (cid, ids) in groups {
        let entities = ids
            .iter()
            .take(5)
            .map(|id| card(engine, *id))
            .collect::<Result<Vec<_>>>()?;
        let facts = facts_for(engine, &ids, false, engine.clock().now, 5)?;
        let score = query
            .map(|q| {
                ids.iter()
                    .filter_map(|id| {
                        engine
                            .db
                            .sql()
                            .query_row(
                                "SELECT mention_count FROM entities WHERE id=?1",
                                [id],
                                |r| r.get::<_, i64>(0),
                            )
                            .ok()
                    })
                    .sum::<i64>() as f64
                    + q.len() as f64 * 0.0
            })
            .unwrap_or(ids.len() as f64);
        out.push(Community {
            id: cid,
            size: ids.len(),
            entities,
            facts,
            score,
        });
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    out.truncate(k.max(1));
    Ok(OverviewOutput { communities: out })
}
