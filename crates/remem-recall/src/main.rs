//! remem: ReMem v3 long-term memory CLI.

mod html;

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use remem_embed::Embedder as _;
use remem_recall::stub::StubEmbedder;
use remem_recall::RecallEngine;
use remem_store::{Store, MAX_LIMIT};
use remem_types::{MemoryItem, MemoryKind, RecallQuery};

/// ReMem v3 memory engine.
#[derive(Parser)]
#[command(name = "remem", version, about)]
struct Cli {
    /// SQLite database file
    #[arg(
        long,
        env = "REMEM_DB",
        default_value = "~/.remem/remem.db",
        global = true
    )]
    db: PathBuf,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Store a memory
    Remember {
        /// Memory kind: fact|decision|mistake|preference|event|note
        kind: String,
        /// memory content (joined with spaces)
        #[arg(required = true)]
        text: Vec<String>,
        /// Comma-separated tags to attach
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Agent id that owns the memory
        #[arg(long, default_value = "")]
        agent: String,
        /// Session id that owns the memory
        #[arg(long, default_value = "")]
        session: String,
        /// When it happened: unix seconds or YYYY-MM-DD (default: storage time)
        #[arg(long)]
        occurred_at: Option<String>,
    },
    /// Ranked recall for a query
    Recall {
        /// Search text (joined with spaces)
        #[arg(required = true)]
        query: Vec<String>,
        /// Max hits to return (default: 5)
        #[arg(long, default_value_t = 5)]
        k: usize,
        /// Print results as pretty JSON
        #[arg(long)]
        json: bool,
        /// Only memories from this agent
        #[arg(long)]
        agent: Option<String>,
        /// Only memories from this session
        #[arg(long)]
        session: Option<String>,
        /// Only memories whose event time is >= this (unix seconds or YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,
        /// Only memories whose event time is <= this (unix seconds or YYYY-MM-DD)
        #[arg(long)]
        until: Option<String>,
        /// Character budget for the returned results
        #[arg(long)]
        max_chars: Option<usize>,
        /// Drop hits scoring below this (0 = off, the default)
        #[arg(long, env = "REMEM_COSINE_FLOOR",
              default_value_t = remem_recall::DEFAULT_MIN_SCORE)]
        min_score: f64,
        /// Filter by kind (fact|decision|mistake|preference|event|note).
        /// Repeatable and comma-separated; kinds OR together.
        #[arg(long, value_delimiter = ',')]
        kind: Vec<String>,
        /// Filter by tag. Repeatable; a memory needs any one of them.
        #[arg(long, value_delimiter = ',')]
        tag: Vec<String>,
        /// Print per-hit per-channel contributions to stderr
        #[arg(long)]
        explain: bool,
        /// Disable the recency band (score contribution + `recent` reasons)
        #[arg(long, env = "REMEM_NO_RECENCY")]
        no_recency: bool,
        /// Recency half-life in days (must be finite and > 0)
        #[arg(long, env = "REMEM_HALF_LIFE_DAYS",
              default_value_t = remem_recall::DEFAULT_HALF_LIFE_DAYS)]
        half_life_days: f64,
    },
    /// List stored memories (newest first)
    List {
        /// Print memories as pretty JSON
        #[arg(long)]
        json: bool,
        /// Max memories to show (newest first, unbounded by default).
        /// Matches recall `k`: 0 shows none, values over 1000 clamp to 1000,
        /// negatives rejected by clap.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Soft-delete a memory (row kept, graph node and edges dropped)
    Forget {
        /// Id of the memory to forget
        id: String,
    },
    /// Hard-delete a memory (row, FTS entry, embedding, graph node)
    Purge {
        /// Id of the memory to purge
        id: String,
    },
    /// Link two memories in the graph
    Link {
        /// Id of the source memory
        from: String,
        /// Id of the target memory
        to: String,
        /// Relation label (default: RELATES_TO)
        #[arg(long)]
        rel: Option<String>,
        /// Edge provenance: manual|recall-suggested|correction-chain (default: manual)
        #[arg(long)]
        provenance: Option<String>,
    },
    /// Correct a memory: supersede it with a new version, linked by a
    /// correction-chain SUPERSEDES edge (prints the new id)
    Correct {
        /// Id of the memory being corrected
        id: String,
        /// Memory kind for the new version: fact|decision|mistake|preference|event|note
        kind: String,
        /// New version content (joined with spaces)
        #[arg(required = true)]
        text: Vec<String>,
        /// Comma-separated tags to attach
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        /// Agent id that owns the memory
        #[arg(long, default_value = "")]
        agent: String,
        /// Session id that owns the memory
        #[arg(long, default_value = "")]
        session: String,
        /// When it happened: unix seconds or YYYY-MM-DD (default: storage time)
        #[arg(long)]
        occurred_at: Option<String>,
    },
    /// Graph neighbours of a memory as `id  rel` lines (Memory nodes only)
    Related {
        /// Id of the memory to show neighbours for
        id: String,
        /// Only edges of this type
        #[arg(long)]
        rel: Option<String>,
    },
    /// Top memories by graph PageRank as `id  score` lines
    Central {
        /// Max memories to show (default: 10)
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Correction chain of a memory, oldest -> newest (empty if unknown)
    Trace {
        /// Id of any version in the chain
        id: String,
    },
    /// What breaks if a memory changes: directional BFS over memory edges
    /// (dependents + dependencies, SUPERSEDES chains included), as an
    /// indented tree
    Impact {
        /// Id of the memory to assess
        id: String,
        /// Max hops from the seed (default: 3)
        #[arg(long, default_value_t = 3)]
        depth: usize,
        /// Only follow edges of this relation type
        #[arg(long)]
        rel: Option<String>,
    },
    /// Replace a memory with a corrected version (soft-supersede + back-pointer)
    Supersede {
        /// Id of the memory to replace
        old_id: String,
        /// Memory kind of the replacement: fact|decision|mistake|preference|event|note
        kind: String,
        /// Replacement content (joined with spaces)
        #[arg(required = true)]
        text: Vec<String>,
    },
    /// Shortest memory-to-memory path as `from` .. `to` lines (empty if unreachable)
    Path {
        /// Id of the start memory
        from: String,
        /// Id of the end memory
        to: String,
    },
    /// Export the knowledge graph as a self-contained HTML file
    Graph {
        /// Output file path
        #[arg(long)]
        html: PathBuf,
        /// Include agent/session hub nodes and BELONGS_TO_* edges
        /// (excluded by default: hub spokes drown the semantic edges)
        #[arg(long)]
        with_hubs: bool,
    },
    /// Database and graph counts
    Stats,
    /// Report graph health problems (exit 1 if any). Orphan (unlinked)
    /// memories are completeness notes, not health: opt in with --show-orphans
    /// (still exit 0).
    Validate {
        /// Also list orphan (unlinked) memories; does not affect the exit code
        #[arg(long)]
        show_orphans: bool,
    },
}

/// Parse a unix timestamp or a `YYYY-MM-DD` date (UTC midnight) into seconds.
fn parse_time(s: &str) -> Result<i64> {
    if let Ok(secs) = s.trim().parse::<i64>() {
        return Ok(secs);
    }
    let mut parts = s.trim().split('-');
    let (y, m, d) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(d), None) => (y, m, d),
        _ => return Err(anyhow!("invalid time '{s}' (unix seconds or YYYY-MM-DD)")),
    };
    let parse = |v: &str| {
        v.parse::<i64>()
            .map_err(|_| anyhow!(format!("invalid date '{s}'")))
    };
    let (y, m, d) = (parse(y)?, parse(m)?, parse(d)?);
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=days_in_month(y, m)).contains(&d)
    {
        return Err(anyhow!("invalid date '{s}'"));
    }
    let days = days_from_civil(y, m, d).ok_or_else(|| anyhow!("date out of range '{s}'"))?;
    days.checked_mul(86_400)
        .ok_or_else(|| anyhow!("date out of range '{s}'"))
}

fn days_in_month(y: i64, m: i64) -> i64 {
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        _ => 28,
    }
}

/// Days since the unix epoch for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`), so YYYY-MM-DD needs no date crate. Checked: `i64`
/// extremes yield `None` instead of panicking or wrapping.
fn days_from_civil(y: i64, m: i64, d: i64) -> Option<i64> {
    let y = if m <= 2 { y.checked_sub(1)? } else { y };
    let era = if y >= 0 { y } else { y.checked_sub(399)? } / 400;
    let yoe = y.checked_sub(era.checked_mul(400)?)?;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe
        .checked_mul(365)?
        .checked_add(yoe / 4)?
        .checked_sub(yoe / 100)?
        .checked_add(doy)?;
    era.checked_mul(146_097)?
        .checked_add(doe)?
        .checked_sub(719_468)
}

fn expand(path: &Path) -> Result<PathBuf> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if let Some(rest) = bytes.strip_prefix(b"~/") {
        let home = std::env::var_os("HOME").filter(|h| !h.is_empty()).ok_or_else(|| {
            anyhow!("REMEM_DB uses ~/ but HOME is unset or empty; set HOME or REMEM_DB to an absolute path")
        })?;
        let mut p = PathBuf::from(home);
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            p.push(std::ffi::OsStr::from_bytes(rest));
        }
        #[cfg(not(unix))]
        {
            p.push(String::from_utf8_lossy(rest).as_ref());
        }
        return Ok(p);
    }
    Ok(path.to_path_buf())
}

/// Adapter: real local embedder behind recall's minimal Embed trait.
struct RealEmbedder(remem_embed::LocalEmbedder);

impl remem_recall::Embed for RealEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.0.embed(texts)
    }
}

fn engine(db: &Path) -> Result<RecallEngine> {
    let path = expand(db)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create db parent dir {}", parent.display()))?;
        }
    }
    let store = Store::open_path(&path).context("open store")?;
    // Graph projection needs a real file so both connections see the same db.
    let graph = remem_graph::Graph::open(&path).map_err(|e| anyhow!("open graph: {e}"))?;
    let embed: Box<dyn remem_recall::Embed> = match remem_embed::load() {
        Ok(m) => {
            eprintln!("embedder: {} ({}d)", m.device_name(), m.dims());
            Box::new(RealEmbedder(m))
        }
        Err(e) => {
            eprintln!("embedder: local model unavailable ({e:#}), using stub");
            Box::new(StubEmbedder)
        }
    };
    Ok(RecallEngine::new(store, embed).with_graph(graph))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Remember {
            kind,
            text,
            tags,
            agent,
            session,
            occurred_at,
        } => {
            let kind = MemoryKind::parse(&kind).ok_or_else(|| {
                anyhow!("unknown kind '{kind}' (fact|decision|mistake|preference|event|note)")
            })?;
            let mut item = MemoryItem::new(kind, text.join(" "));
            item.tags = tags;
            item.agent_id = agent;
            item.session_id = session;
            if let Some(when) = occurred_at.as_deref() {
                item.occurred_at = Some(parse_time(when)?);
            }
            let eng = engine(&cli.db)?;
            // Check before the write: an existing row with the same hash means
            // insert() will dedup (merged), not insert (split).
            let hash_dup = eng
                .store()
                .find_by_hash(&remem_store::content_hash(&kind, &item.content))
                .is_some();
            let (id, similar) = eng.remember(&item)?;
            // Line 1 stays the bare id: eval.sh and scripts take stdout line 0.
            println!("{id}");
            // A merge returns an id that already existed before this write
            // (exact hash dup via find_by_hash, or an absorbed near-duplicate
            // whose old id is among the similar candidates); a split is new.
            if hash_dup || similar.iter().any(|(sid, _)| *sid == id) {
                eprintln!("merged: {id}");
            } else {
                eprintln!("split: {id}");
            }
            if !similar.is_empty() {
                let list: Vec<String> = similar
                    .iter()
                    .map(|(sid, d)| format!("{sid} {d:.3}"))
                    .collect();
                eprintln!("similar: [{}]", list.join(", "));
            }
        }
        Cmd::Recall {
            query,
            k,
            json,
            agent,
            session,
            since,
            until,
            max_chars,
            min_score,
            kind,
            tag,
            explain,
            no_recency,
            half_life_days,
        } => {
            let parse_kind = |s: &str| {
                MemoryKind::parse(s).ok_or_else(|| {
                    anyhow!("unknown kind '{s}' (fact|decision|mistake|preference|event|note)")
                })
            };
            let kinds = if kind.is_empty() {
                None
            } else {
                Some(
                    kind.iter()
                        .map(|s| parse_kind(s))
                        .collect::<Result<Vec<_>>>()?,
                )
            };
            let tags = if tag.is_empty() { None } else { Some(tag) };
            if !half_life_days.is_finite() || half_life_days <= 0.0 {
                return Err(anyhow!(
                    "invalid --half-life-days '{half_life_days}' (must be finite and > 0)"
                ));
            }
            let q = RecallQuery {
                text: query.join(" "),
                k,
                max_chars,
                agent_id: agent,
                session_id: session,
                since: since.as_deref().map(parse_time).transpose()?,
                until: until.as_deref().map(parse_time).transpose()?,
                kinds,
                tags,
                ..Default::default()
            };
            let eng = engine(&cli.db)?
                .with_min_score(min_score)
                .with_half_life_days(half_life_days);
            let eng = if no_recency {
                eng.with_recency_off()
            } else {
                eng
            };
            let hits = eng.recall(&q)?;
            if explain {
                // Contributions are weight * 1/(DEFAULT_RRF_K + rank), parsed
                // from the per-hit reasons ("fts#1", "vector#2", "graph#3").
                for h in &hits {
                    let mut parts: Vec<String> = Vec::new();
                    for r in &h.reasons {
                        if let Some((ch, rank)) = r.split_once('#') {
                            if let (Some(w), Ok(rank)) = (
                                match ch {
                                    "fts" => Some(eng.weights().fts),
                                    "vector" => Some(eng.weights().vector),
                                    "graph" => Some(eng.weights().graph),
                                    _ => None,
                                },
                                rank.parse::<usize>(),
                            ) {
                                parts.push(format!(
                                    "{ch}#{rank}={:.5}",
                                    w / (remem_recall::DEFAULT_RRF_K as f64 + rank as f64)
                                ));
                            }
                        }
                    }
                    if !parts.is_empty() {
                        eprintln!("{}  {}", h.item.id, parts.join(" "));
                    }
                }
            }
            if json {
                let v: Vec<_> = hits
                    .iter()
                    .map(|h| {
                        serde_json::json!({
                            "id": h.item.id,
                            "kind": h.item.kind.as_str(),
                            "content": h.item.content,
                            "tags": h.item.tags,
                            "agent": h.item.agent_id,
                            "session": h.item.session_id,
                            "occurred_at": h.item.occurred_at,
                            "importance": h.item.importance,
                            "score": h.score,
                            "reasons": h.reasons,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                for h in hits {
                    println!(
                        "{}  {:.4}  [{}]  {}  ({})",
                        h.item.id,
                        h.score,
                        h.item.kind.as_str(),
                        h.item.content,
                        h.reasons.join(",")
                    );
                }
            }
        }
        Cmd::List { json, limit } => {
            let mut items = engine(&cli.db)?.list()?;
            if let Some(n) = limit {
                items.truncate(n.min(MAX_LIMIT as usize));
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else {
                for m in items {
                    println!("{}  [{}]  {}", m.id, m.kind.as_str(), m.content);
                }
            }
        }
        Cmd::Forget { id } => {
            let eng = engine(&cli.db)?;
            // Soft-delete, mirroring MCP forget: an unknown id is a no-op, not
            // an error, and reports forgotten: false.
            let forgotten = eng.store().get(&id).context("forget")?.is_some();
            if forgotten {
                eng.forget(&id)?;
                println!("forgot {id}");
            }
        }
        Cmd::Purge { id } => {
            let path = expand(&cli.db)?;
            let store = Store::open_path(&path).context("open store")?;
            if !store.purge(&id).context("purge")? {
                return Err(anyhow!("unknown id '{id}'"));
            }
            let graph = remem_graph::Graph::open(&path).map_err(|e| anyhow!("open graph: {e}"))?;
            graph
                .forget(&id)
                .map_err(|e| anyhow!("graph forget: {e}"))?;
            println!("purged {id}");
        }
        Cmd::Link {
            from,
            to,
            rel,
            provenance,
        } => {
            let eng = engine(&cli.db)?;
            match provenance.as_deref() {
                None => eng.link(&from, &to, rel.as_deref())?,
                Some(p) => eng.link_with_provenance(&from, &to, rel.as_deref(), p)?,
            }
        }
        Cmd::Correct {
            id,
            kind,
            text,
            tags,
            agent,
            session,
            occurred_at,
        } => {
            let kind = MemoryKind::parse(&kind).ok_or_else(|| {
                anyhow!("unknown kind '{kind}' (fact|decision|mistake|preference|event|note)")
            })?;
            let mut item = MemoryItem::new(kind, text.join(" "));
            item.tags = tags;
            item.agent_id = agent;
            item.session_id = session;
            if let Some(when) = occurred_at.as_deref() {
                item.occurred_at = Some(parse_time(when)?);
            }
            let new_id = engine(&cli.db)?.supersede(&id, &item)?;
            println!("{new_id}");
        }
        Cmd::Related { id, rel } => {
            // Unknown id stays empty: neighbours of nothing is empty, not an
            // error (unlike link, which must resolve both endpoints).
            let eng = engine(&cli.db)?;
            let g = eng.graph().context("engine has no graph open")?;
            let filter = rel.unwrap_or_default();
            let mut hits: Vec<(String, String, String)> = g
                .neighbors_detail(&id)
                .map_err(|e| anyhow!("graph neighbors: {e}"))?
                .into_iter()
                .filter(|n| {
                    n.labels.iter().any(|l| l == remem_graph::MEMORY_LABEL)
                        && (filter.is_empty() || n.rel == filter)
                })
                .map(|n| (n.id, n.rel, n.provenance.to_string()))
                .collect();
            hits.sort();
            for (nid, r, p) in hits {
                println!("{nid}  {r}  {p}");
            }
        }
        Cmd::Central { limit } => {
            let eng = engine(&cli.db)?;
            let g = eng.graph().context("engine has no graph open")?;
            for (id, score) in g
                .central()
                .map_err(|e| anyhow!("graph central: {e}"))?
                .into_iter()
                .take(limit)
            {
                // Provenance summary over memory-to-memory edges: distinct
                // values, or manual when there is nothing unreviewed to show.
                let mut provs: Vec<String> = g
                    .neighbors_detail(&id)
                    .map_err(|e| anyhow!("graph neighbors: {e}"))?
                    .into_iter()
                    .filter(|n| n.labels.iter().any(|l| l == remem_graph::MEMORY_LABEL))
                    .map(|n| n.provenance.to_string())
                    .collect();
                provs.sort();
                provs.dedup();
                let show = match provs.is_empty() {
                    true => "manual".to_string(),
                    false => provs.join(","),
                };
                println!("{id}  {score:.6}  {show}");
            }
        }
        Cmd::Supersede { old_id, kind, text } => {
            let kind = MemoryKind::parse(&kind).ok_or_else(|| {
                anyhow!("unknown kind '{kind}' (fact|decision|mistake|preference|event|note)")
            })?;
            let eng = engine(&cli.db)?;
            let mut item = MemoryItem::new(kind, text.join(" "));
            // Carry forward the agent/session ownership of the old row.
            if let Some(old) = eng.store().get(&old_id)? {
                item.agent_id = old.agent_id;
                item.session_id = old.session_id;
            }
            let id = eng.store().supersede(&old_id, &item).context("supersede")?;
            println!("{id}");
        }
        Cmd::Trace { id } => {
            // Unknown id -> empty chain, mirroring related/path emptiness.
            let eng = engine(&cli.db)?;
            for cid in eng.store().trace(&id).context("trace")? {
                println!("{cid}");
            }
            // Traceable merges (issue #14): absorbed rows under the survivor.
            for content in eng.store().merge_trace(&id).context("trace merges")? {
                println!("merged: {content}");
            }
        }
        Cmd::Impact { id, depth, rel } => {
            // Unknown id stays an empty tree: impact of nothing touches
            // nothing (mirrors related/path/trace emptiness).
            let eng = engine(&cli.db)?;
            let report = match rel.as_deref() {
                Some(r) => eng.impact_filtered(&id, depth, Some(r))?,
                None => eng.impact_depth(&id, depth)?,
            };
            // Snippets come per-id from the store (get_any: superseded rows
            // surface too); a forgotten memory's graph node shows no text.
            let snippet = |c: &str| {
                let mut s: String = c.chars().take(80).collect();
                if c.chars().count() > 80 {
                    s.push('…');
                }
                s
            };
            let row = |nid: &str| -> (String, String) {
                match eng.store().get_any(nid) {
                    Ok(Some(m)) => (m.kind.as_str().to_string(), snippet(&m.content)),
                    _ => (String::new(), String::new()),
                }
            };
            let (root_kind, root_text) = row(&id);
            println!("{id}  [{root_kind}]  {root_text}");
            let mut order: Vec<&remem_recall::ImpactNode> = report.nodes.iter().collect();
            order.sort_by_key(|n| (n.depth, n.id.clone()));
            for n in order {
                let (kind, text) = row(&n.id);
                let indent = "  ".repeat(n.depth);
                println!(
                    "{indent}<- {nid}  [{kind}]  {rel} ({prov})  {text}",
                    nid = n.id,
                    rel = n.rel,
                    prov = n.provenance
                );
            }
        }
        Cmd::Path { from, to } => {
            let eng = engine(&cli.db)?;
            let g = eng.graph().context("engine has no graph open")?;
            for id in g
                .shortest_path(&from, &to)
                .map_err(|e| anyhow!("graph path: {e}"))?
            {
                println!("{id}");
            }
        }
        Cmd::Graph { html, with_hubs } => {
            let eng = engine(&cli.db)?;
            let g = eng.graph().context("engine has no graph open")?;
            let (nodes, edges) = g.edges().map_err(|e| anyhow!("graph edges: {e}"))?;
            // Content snippets from the store; hub nodes have no memory row.
            let items = eng.store().list(false).context("list memories")?;
            let by_id: std::collections::HashMap<&str, &MemoryItem> =
                items.iter().map(|m| (m.id.as_str(), m)).collect();
            let snippet = |id: &str| {
                by_id
                    .get(id)
                    .map(|m| {
                        let mut s = m.content.chars().take(200).collect::<String>();
                        if m.content.chars().count() > 200 {
                            s.push('…');
                        }
                        s
                    })
                    .unwrap_or_default()
            };
            let hub = |id: &str| {
                id.starts_with(remem_graph::AGENT_PREFIX)
                    || id.starts_with(remem_graph::SESSION_PREFIX)
            };
            let nodes: Vec<_> = if with_hubs {
                nodes
            } else {
                nodes.into_iter().filter(|n| !hub(&n.id)).collect()
            };
            let hub_count = nodes.iter().filter(|n| hub(&n.id)).count();
            let edges: Vec<_> = if with_hubs {
                edges
            } else {
                edges
                    .into_iter()
                    .filter(|e| !hub(&e.from) && !hub(&e.to))
                    .collect()
            };
            let node_json: Vec<_> = nodes
                .iter()
                .map(|n| {
                    serde_json::json!({
                        "id": n.id, "kind": n.kind, "snippet": snippet(&n.id),
                    })
                })
                .collect();
            let edge_json: Vec<_> = edges
                .iter()
                .map(|e| serde_json::json!({"from": e.from, "rel": e.rel, "to": e.to}))
                .collect();
            let doc = html::graph_html(
                &serde_json::to_value(&node_json)?,
                &serde_json::to_value(&edge_json)?,
            )?;
            std::fs::write(&html, doc).with_context(|| format!("write {}", html.display()))?;
            let hubs = if with_hubs {
                format!(", {} hubs", hub_count)
            } else {
                String::new()
            };
            println!(
                "wrote {} ({} nodes{hubs}, {} edges)",
                html.display(),
                nodes.len() - hub_count,
                edges.len()
            );
        }
        Cmd::Stats => {
            println!(
                "{}",
                serde_json::to_string_pretty(&engine(&cli.db)?.stats()?)?
            );
        }
        Cmd::Validate { show_orphans } => {
            let eng = engine(&cli.db)?;
            let graph = eng.graph().context("engine has no graph open")?;
            let mut problems = graph.validate();
            problems.extend(eng.graph_gaps().context("graph gaps")?);
            problems.extend(eng.suspect_supersedes().context("suspect supersedes")?);
            for line in &problems {
                println!("{line}");
            }
            if show_orphans {
                match graph.orphan_memories() {
                    Ok(notes) => {
                        for line in &notes {
                            println!("{line}");
                        }
                    }
                    Err(e) => println!("validation query failed: {e}"),
                }
            }
            if !problems.is_empty() {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod html_tests {
    use super::html::graph_html;

    #[test]
    fn html_is_self_contained_and_embeds_data() -> anyhow::Result<()> {
        let nodes = serde_json::json!([
            {"id": "m1", "kind": "fact", "snippet": "hello world"},
            {"id": "agent:a1", "kind": "Agent", "snippet": "hub agent:a1"}
        ]);
        let edges = serde_json::json!([
            {"from": "m1", "rel": "BELONGS_TO_AGENT", "to": "agent:a1"}
        ]);
        let html = graph_html(&nodes, &edges)?;
        // self-contained: no external references, data embedded as JSON
        assert!(!html.contains("http://") && !html.contains("https://"));
        assert!(!html.contains("<script src="));
        assert!(html.contains("agent:a1"));
        assert!(html.contains("<canvas"));
        assert!(html.contains("filter")); // kind filter control
        Ok(())
    }
}

#[cfg(test)]
mod time_tests {
    use super::{days_from_civil, parse_time};

    #[test]
    fn unix_epoch_seconds_and_days() {
        assert_eq!(parse_time("0").unwrap(), 0);
        assert_eq!(parse_time("1700000000").unwrap(), 1_700_000_000);
        assert_eq!(days_from_civil(1970, 1, 1), Some(0));
        assert_eq!(parse_time("1970-01-01").unwrap(), 0);
    }

    #[test]
    fn leap_day_ok_and_non_leap_rejected() {
        assert_eq!(parse_time("2020-02-29").unwrap(), 1_582_934_400);
        assert!(parse_time("2019-02-29").is_err());
        assert!(parse_time("2100-02-29").is_err());
        assert!(parse_time("2000-02-29").is_ok());
    }

    #[test]
    fn year_out_of_range_rejected() {
        assert!(parse_time("0000-01-01").is_err());
        assert!(parse_time("10000-01-01").is_err());
        assert!(parse_time("9999-12-31").is_ok());
        assert!(parse_time("0001-01-01").is_ok());
    }

    #[test]
    fn extreme_input_does_not_panic() {
        assert_eq!(parse_time(&i64::MAX.to_string()).unwrap(), i64::MAX);
        assert_eq!(parse_time(&i64::MIN.to_string()).unwrap(), i64::MIN);
        assert!(days_from_civil(i64::MAX, 1, 1).is_none());
        assert!(days_from_civil(i64::MIN, 12, 31).is_none());
        assert!(days_from_civil(0, 3, 1).unwrap() < days_from_civil(1, 3, 1).unwrap());
    }

    #[test]
    fn malformed_rejected() {
        for s in [
            "2020-13-01",
            "2020-00-10",
            "2020-04-31",
            "2020-01-0",
            "2020-01-01-01",
            "abc",
            "-1-1-1",
        ] {
            assert!(parse_time(s).is_err(), "expected error for {s:?}");
        }
    }
}

#[cfg(test)]
mod path_tests {
    use super::{engine, expand};
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static L: OnceLock<Mutex<()>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        _g: std::sync::MutexGuard<'static, ()>,
        old_home: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn lock() -> Self {
            let g = env_lock().lock().unwrap();
            Self {
                _g: g,
                old_home: std::env::var_os("HOME"),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_path_is_preserved() {
        use std::os::unix::ffi::OsStrExt;
        let raw = std::ffi::OsStr::from_bytes(b"/tmp/remem-nonutf8-\xff.db");
        let p = expand(PathBuf::from(raw).as_path()).expect("expand keeps non-UTF8");
        assert_eq!(p.as_os_str().as_bytes(), b"/tmp/remem-nonutf8-\xff.db");
        assert!(!p.as_os_str().is_empty());
    }

    #[test]
    fn home_unset_with_tilde_errors_clearly() {
        let _env = EnvGuard::lock();
        std::env::remove_var("HOME");
        let err = format!(
            "{:?}",
            expand(PathBuf::from("~/.remem/remem.db").as_path()).unwrap_err()
        );
        assert!(err.contains("HOME"), "error must name HOME: {err}");
        let eng_err = format!(
            "{:?}",
            engine(PathBuf::from("~/.remem/remem.db").as_path())
                .err()
                .expect("engine must fail")
        );
        assert!(eng_err.contains("HOME"), "engine must propagate: {eng_err}");
    }

    #[test]
    fn create_dir_errors_propagate_with_context() {
        let dir = std::env::temp_dir().join(format!(
            "remem-recall-blocker-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let db = blocker.join("remem.db");
        let err = format!("{:#}", engine(&db).err().expect("engine must fail"));
        assert!(
            err.contains("create db parent dir"),
            "must carry context: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tilde_expands_under_home() {
        let _env = EnvGuard::lock();
        let home = std::env::temp_dir().join(format!(
            "remem-recall-home-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("HOME", &home);
        let p = expand(PathBuf::from("~/.remem/remem.db").as_path()).expect("tilde expands");
        assert_eq!(p, home.join(".remem/remem.db"));
        assert!(!p.to_string_lossy().starts_with('~'));
        let _ = std::fs::remove_dir_all(&home);
    }
}
