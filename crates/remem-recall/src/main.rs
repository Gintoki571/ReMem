//! remem: ReMem v3 long-term memory CLI.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use remem_embed::Embedder as _;
use remem_recall::stub::StubEmbedder;
use remem_recall::RecallEngine;
use remem_store::Store;
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
    db: String,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Store a memory
    Remember {
        kind: String,
        /// memory content (joined with spaces)
        #[arg(required = true)]
        text: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        tags: Vec<String>,
        #[arg(long, default_value = "")]
        agent: String,
        #[arg(long, default_value = "")]
        session: String,
        #[arg(long, default_value_t = 0.5)]
        importance: f32,
    },
    /// Ranked recall for a query
    Recall {
        #[arg(required = true)]
        query: Vec<String>,
        #[arg(long, default_value_t = 5)]
        k: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        session: Option<String>,
    },
    /// List stored memories (newest first)
    List {
        #[arg(long)]
        json: bool,
    },
    /// Link two memories in the graph
    Link {
        from: String,
        to: String,
        #[arg(long)]
        rel: Option<String>,
    },
    /// Database and graph counts
    Stats,
    /// Report dangling graph edges and orphan memories (exit 1 if any)
    Validate,
}

fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

/// Adapter: real local embedder behind recall's minimal Embed trait.
struct RealEmbedder(remem_embed::LocalEmbedder);

impl remem_recall::Embed for RealEmbedder {
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.0.embed(texts)
    }
}

fn engine(db: &str) -> Result<RecallEngine> {
    let path = expand(db);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
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
            importance,
        } => {
            let kind = MemoryKind::parse(&kind).ok_or_else(|| {
                anyhow!("unknown kind '{kind}' (fact|decision|mistake|preference|event|note)")
            })?;
            let mut item = MemoryItem::new(kind, text.join(" "));
            item.tags = tags;
            item.agent_id = agent;
            item.session_id = session;
            item.importance = importance;
            let id = engine(&cli.db)?.remember(&item)?;
            println!("{id}");
        }
        Cmd::Recall {
            query,
            k,
            json,
            agent,
            session,
        } => {
            let q = RecallQuery {
                text: query.join(" "),
                k,
                agent_id: agent,
                session_id: session,
                ..Default::default()
            };
            let hits = engine(&cli.db)?.recall(&q)?;
            if json {
                let v: Vec<_> = hits
                    .iter()
                    .map(|h| {
                        serde_json::json!({
                            "id": h.item.id,
                            "kind": h.item.kind.as_str(),
                            "content": h.item.content,
                            "tags": h.item.tags,
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
        Cmd::List { json } => {
            let items = engine(&cli.db)?.list()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else {
                for m in items {
                    println!("{}  [{}]  {}", m.id, m.kind.as_str(), m.content);
                }
            }
        }
        Cmd::Link { from, to, rel } => {
            engine(&cli.db)?.link(&from, &to, rel.as_deref())?;
        }
        Cmd::Stats => {
            println!(
                "{}",
                serde_json::to_string_pretty(&engine(&cli.db)?.stats()?)?
            );
        }
        Cmd::Validate => {
            let problems = engine(&cli.db)?
                .graph()
                .context("engine has no graph open")?
                .validate();
            for line in &problems {
                println!("{line}");
            }
            if !problems.is_empty() {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
