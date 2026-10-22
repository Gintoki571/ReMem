//! CLI: hand-rolled argument parsing, one subcommand per MCP tool plus
//! `mcp`, `reindex`, `validate`, `cypher` and `import-remem`.
//!
//! Exit codes: 0 ok, 1 runtime error, 2 usage error.

use neocortex::embed::{Embedder, HashEmbedder};
use neocortex::engine::{Engine, EntityIn, RecallInput, RelationIn, RememberInput};
use neocortex::mcp::LazyEmbedder;
use neocortex::render;
use neocortex::{import, mcp, Error};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug)]
enum Fail {
    Usage(String),
    Run(String),
}

impl Fail {
    fn code(&self) -> i32 {
        match self {
            Fail::Usage(_) => 2,
            Fail::Run(_) => 1,
        }
    }

    fn message(&self) -> &str {
        match self {
            Fail::Usage(m) | Fail::Run(m) => m,
        }
    }
}

fn usage(msg: impl Into<String>) -> Fail {
    Fail::Usage(msg.into())
}

fn runtime(e: Error) -> Fail {
    Fail::Run(e.to_string())
}

type CmdResult = std::result::Result<(), Fail>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(fail) = dispatch(&args) {
        eprintln!("neocortex: {}", fail.message());
        std::process::exit(fail.code());
    }
}

struct Globals {
    db: Option<String>,
    json: bool,
}

fn dispatch(args: &[String]) -> CmdResult {
    let (globals, start) = parse_globals(args)?;
    let rest = &args[start..];
    let Some(command) = rest.first() else {
        print_help();
        return Ok(());
    };
    let rest = &rest[1..];
    match command.as_str() {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "--version" | "-V" | "version" => {
            println!("neocortex {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "remember" => cmd_remember(&globals, rest),
        "recall" => cmd_recall(&globals, rest),
        "entity" => cmd_entity(&globals, rest),
        "invalidate" => cmd_invalidate(&globals, rest),
        "forget" => cmd_forget(&globals, rest),
        "merge" => cmd_merge(&globals, rest),
        "alias" => cmd_alias(&globals, rest),
        "overview" => cmd_overview(&globals, rest),
        "stats" => cmd_stats(&globals),
        "reindex" => cmd_reindex(&globals),
        "validate" => cmd_validate(&globals),
        "cypher" => cmd_cypher(&globals, rest),
        "import-remem" => cmd_import(&globals, rest),
        "mcp" => {
            let engine = open_engine(&globals)?;
            mcp::serve(&engine).map_err(runtime)
        }
        other => Err(usage(format!(
            "unknown command: {other}\nrun `neocortex help` for usage"
        ))),
    }
}

fn parse_globals(args: &[String]) -> std::result::Result<(Globals, usize), Fail> {
    let mut globals = Globals {
        db: None,
        json: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--db" => {
                globals.db = Some(value(args, &mut i, "--db")?);
            }
            "--json" => globals.json = true,
            _ => break,
        }
        i += 1;
    }
    Ok((globals, i))
}

fn value(args: &[String], i: &mut usize, flag: &str) -> std::result::Result<String, Fail> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| usage(format!("{flag} needs a value")))
}

fn list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `--entities` / `--relations`: a JSON array or one object.
fn json_list<T: DeserializeOwned>(raw: &str, flag: &str) -> std::result::Result<Vec<T>, Fail> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| usage(format!("{flag}: invalid JSON: {e}")))?;
    match value {
        serde_json::Value::Array(_) => {
            serde_json::from_value(value).map_err(|e| usage(format!("{flag}: {e}")))
        }
        serde_json::Value::Object(_) => serde_json::from_value(value)
            .map(|item| vec![item])
            .map_err(|e| usage(format!("{flag}: {e}"))),
        _ => Err(usage(format!("{flag} needs a JSON object or array"))),
    }
}

fn db_path(globals: &Globals) -> std::result::Result<PathBuf, Fail> {
    if let Some(path) = &globals.db {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("NEOCORTEX_DB") {
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let Some(home) = std::env::var_os("HOME") else {
        return Err(Fail::Run(
            "no database path: set NEOCORTEX_DB or pass --db".to_string(),
        ));
    };
    Ok(PathBuf::from(home).join(".neocortex").join("neocortex.db"))
}

fn open_engine(globals: &Globals) -> std::result::Result<Engine, Fail> {
    let path = db_path(globals)?;
    let embedder: Arc<dyn Embedder> = match std::env::var("NEOCORTEX_EMBEDDER").as_deref() {
        Ok("hash") => Arc::new(HashEmbedder),
        _ => Arc::new(LazyEmbedder::new()),
    };
    Engine::open(&path, embedder).map_err(runtime)
}

fn emit<T: Serialize>(globals: &Globals, out: &T, text: impl FnOnce() -> String) -> CmdResult {
    if globals.json {
        println!(
            "{}",
            serde_json::to_string_pretty(out).map_err(|e| Fail::Run(e.to_string()))?
        );
    } else {
        print!("{}", text());
    }
    Ok(())
}

// ---------------------------------------------------------------- commands

fn cmd_remember(globals: &Globals, args: &[String]) -> CmdResult {
    let input = parse_remember(args)?;
    let engine = open_engine(globals)?;
    let out = engine.remember(input).map_err(runtime)?;
    emit(globals, &out, || render::render_remember(&out))
}

fn parse_remember(args: &[String]) -> std::result::Result<RememberInput, Fail> {
    let mut input = RememberInput::default();
    let mut words: Vec<String> = Vec::new();
    let mut from_stdin = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "-" => from_stdin = true,
            "--kind" => input.kind = Some(value(args, &mut i, arg)?),
            "--tags" => input.tags = list(&value(args, &mut i, arg)?),
            "--importance" => {
                let raw = value(args, &mut i, arg)?;
                input.importance = Some(
                    raw.parse()
                        .map_err(|_| usage("--importance needs a number 0..1"))?,
                );
            }
            "--scope" => input.scope = Some(value(args, &mut i, arg)?),
            "--source" => input.source = Some(value(args, &mut i, arg)?),
            "--session" => input.session = Some(value(args, &mut i, arg)?),
            "--when" => input.valid_from = Some(value(args, &mut i, arg)?),
            "--until" => input.valid_to = Some(value(args, &mut i, arg)?),
            "--entities" => {
                let raw = value(args, &mut i, arg)?;
                input.entities = json_list::<EntityIn>(&raw, "--entities")?;
            }
            "--relations" => {
                let raw = value(args, &mut i, arg)?;
                input.relations = json_list::<RelationIn>(&raw, "--relations")?;
            }
            "--keywords" => input.keywords = list(&value(args, &mut i, arg)?),
            "--supersedes" => input.supersedes = list(&value(args, &mut i, arg)?),
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option {other}")));
            }
            _ => words.push(args[i].clone()),
        }
        i += 1;
    }
    if from_stdin {
        let mut raw = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw)
            .map_err(|e| Fail::Run(format!("cannot read stdin: {e}")))?;
        return serde_json::from_str(&raw)
            .map_err(|e| usage(format!("invalid RememberInput JSON on stdin: {e}")));
    }
    if words.is_empty() {
        return Err(usage(
            "remember needs content, or `remember -` to read a JSON object from stdin",
        ));
    }
    input.content = words.join(" ");
    Ok(input)
}

fn cmd_recall(globals: &Globals, args: &[String]) -> CmdResult {
    let input = parse_recall(args)?;
    let engine = open_engine(globals)?;
    let out = engine.recall(&input).map_err(runtime)?;
    emit(globals, &out, || {
        render::render_recall(&out, &render::clock_fmt(&engine.clock()))
    })
}

fn parse_recall(args: &[String]) -> std::result::Result<RecallInput, Fail> {
    let mut input = RecallInput::default();
    let mut words: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--k" => input.k = Some(number(args, &mut i, arg)?),
            "--budget" => input.budget = Some(number(args, &mut i, arg)?),
            "--kinds" => input.kinds = list(&value(args, &mut i, arg)?),
            "--tags" => input.tags = list(&value(args, &mut i, arg)?),
            "--scope" => input.scope = Some(value(args, &mut i, arg)?),
            "--source" => input.source = Some(value(args, &mut i, arg)?),
            "--session" => input.session = Some(value(args, &mut i, arg)?),
            "--since" => input.since = Some(value(args, &mut i, arg)?),
            "--until" => input.until = Some(value(args, &mut i, arg)?),
            "--as-of" => input.as_of = Some(value(args, &mut i, arg)?),
            "--history" => input.include_history = true,
            "--entities" => input.entities = list(&value(args, &mut i, arg)?),
            "--keywords" => input.keywords = list(&value(args, &mut i, arg)?),
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option {other}")));
            }
            _ => words.push(args[i].clone()),
        }
        i += 1;
    }
    if words.is_empty() {
        return Err(usage("recall needs a query"));
    }
    input.query = words.join(" ");
    Ok(input)
}

fn number(args: &[String], i: &mut usize, flag: &str) -> std::result::Result<usize, Fail> {
    let raw = value(args, i, flag)?;
    raw.parse()
        .map_err(|_| usage(format!("{flag} needs a positive integer")))
}

fn cmd_entity(globals: &Globals, args: &[String]) -> CmdResult {
    let mut name: Option<String> = None;
    let mut limit = 20usize;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--limit" => limit = number(args, &mut i, arg)?,
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option {other}")));
            }
            _ => name = Some(args[i].clone()),
        }
        i += 1;
    }
    let name = name.ok_or_else(|| usage("entity needs a name or e<id>"))?;
    let engine = open_engine(globals)?;
    let out = engine.entity(&name, limit).map_err(runtime)?;
    emit(globals, &out, || {
        render::render_entity(&out, &render::clock_fmt(&engine.clock()))
    })
}

fn cmd_invalidate(globals: &Globals, args: &[String]) -> CmdResult {
    let mut id: Option<String> = None;
    let mut at: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--at" => at = Some(value(args, &mut i, arg)?),
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option {other}")));
            }
            _ => id = Some(args[i].clone()),
        }
        i += 1;
    }
    let id = id.ok_or_else(|| usage("invalidate needs an m<id> or r<id>"))?;
    let engine = open_engine(globals)?;
    let out = engine.invalidate(&id, at.as_deref()).map_err(runtime)?;
    emit(globals, &out, || render::render_change(&out))
}

fn cmd_forget(globals: &Globals, args: &[String]) -> CmdResult {
    let mut id: Option<String> = None;
    let mut hard = false;
    for arg in args {
        match arg.as_str() {
            "--hard" => hard = true,
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option {other}")));
            }
            _ => id = Some(arg.clone()),
        }
    }
    let id = id.ok_or_else(|| usage("forget needs an m<id>"))?;
    let engine = open_engine(globals)?;
    let out = engine.forget(&id, hard).map_err(runtime)?;
    emit(globals, &out, || render::render_change(&out))
}

fn cmd_merge(globals: &Globals, args: &[String]) -> CmdResult {
    let (keep, merge) = two_names(args, "merge <keep> <merge>")?;
    let engine = open_engine(globals)?;
    let out = engine.merge_entities(&keep, &merge).map_err(runtime)?;
    emit(globals, &out, || render::render_change(&out))
}

fn cmd_alias(globals: &Globals, args: &[String]) -> CmdResult {
    let (entity, alias) = two_names(args, "alias <entity> <alias>")?;
    let engine = open_engine(globals)?;
    let out = engine.add_alias(&entity, &alias).map_err(runtime)?;
    emit(globals, &out, || render::render_change(&out))
}

fn two_names(args: &[String], usage_text: &str) -> std::result::Result<(String, String), Fail> {
    if args.len() != 2 || args.iter().any(|a| a.starts_with('-')) {
        return Err(usage(format!("usage: neocortex {usage_text}")));
    }
    Ok((args[0].clone(), args[1].clone()))
}

fn cmd_overview(globals: &Globals, args: &[String]) -> CmdResult {
    let mut words: Vec<String> = Vec::new();
    let mut k = 5usize;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--k" => k = number(args, &mut i, arg)?,
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option {other}")));
            }
            _ => words.push(args[i].clone()),
        }
        i += 1;
    }
    let query = if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    };
    let engine = open_engine(globals)?;
    let out = engine.overview(query.as_deref(), k).map_err(runtime)?;
    emit(globals, &out, || {
        render::render_overview(&out, &render::clock_fmt(&engine.clock()))
    })
}

fn cmd_stats(globals: &Globals) -> CmdResult {
    let engine = open_engine(globals)?;
    let out = engine.stats().map_err(runtime)?;
    emit(globals, &out, || render::render_stats(&out))
}

fn cmd_reindex(globals: &Globals) -> CmdResult {
    let engine = open_engine(globals)?;
    let out = engine.reindex().map_err(runtime)?;
    emit(globals, &out, || render::render_change(&out))
}

fn cmd_validate(globals: &Globals) -> CmdResult {
    let engine = open_engine(globals)?;
    let problems = engine.validate().map_err(runtime)?;
    if problems.is_empty() {
        println!("ok: no problems");
        return Ok(());
    }
    for problem in &problems {
        println!("{problem}");
    }
    Err(Fail::Run(format!("{} problem(s) found", problems.len())))
}

fn cmd_cypher(globals: &Globals, args: &[String]) -> CmdResult {
    if args.is_empty() {
        return Err(usage("cypher needs a query"));
    }
    let engine = open_engine(globals)?;
    let value = engine.cypher(&args.join(" ")).map_err(runtime)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|e| Fail::Run(e.to_string()))?
    );
    Ok(())
}

fn cmd_import(globals: &Globals, args: &[String]) -> CmdResult {
    let path = args
        .first()
        .ok_or_else(|| usage("import-remem needs a path to a ReMem v3 database"))?;
    let engine = open_engine(globals)?;
    let (imported, skipped) = import::import_remem(&engine, Path::new(path)).map_err(runtime)?;
    if globals.json {
        println!(
            "{}",
            serde_json::json!({"imported": imported, "skipped": skipped})
        );
    } else {
        println!("imported {imported}, skipped {skipped}");
    }
    Ok(())
}

fn print_help() {
    println!(
        "neocortex {} — long-term memory for AI agents

Usage: neocortex [--db PATH] [--json] <command> [args]

Commands:
  remember <content...>    store a memory; `remember -` reads a JSON object from stdin
  recall <query...>        retrieve memories, facts and entities
  entity <name|eID>        profile and timeline of one entity
  invalidate <mID|rID>     end a memory or relation at a time, keeping history
  forget <mID>             retire a memory (--hard deletes it)
  merge <keep> <merge>     merge one entity into another
  alias <entity> <alias>   add a name to an entity
  overview [query...]      communities of the entity graph
  stats                    counts, database size and embedder
  reindex                  rebuild the graph projection
  validate                 report integrity problems (exit 1 if any)
  cypher <query>           run read-only Cypher
  import-remem <path>      import a ReMem v3 SQLite file
  mcp                      run the MCP stdio server
  help                     this text
  --version                print the version

Common flags: --db PATH (default $NEOCORTEX_DB), --json (machine-readable output).
remember flags: --kind --tags --importance --scope --source --session --when --until
                --entities JSON --relations JSON --keywords --supersedes
recall flags:   --k --budget --kinds --tags --scope --source --session --since --until
                --as-of --history --entities --keywords",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn remember_collects_words_and_flags() {
        let input = parse_remember(&args(&[
            "Mom",
            "moved",
            "to",
            "Pune",
            "--kind",
            "event",
            "--tags",
            "family, move",
            "--importance",
            "0.7",
            "--supersedes",
            "m3,m4",
        ]))
        .unwrap();
        assert_eq!(input.content, "Mom moved to Pune");
        assert_eq!(input.kind.as_deref(), Some("event"));
        assert_eq!(input.tags, vec!["family", "move"]);
        assert_eq!(input.importance, Some(0.7));
        assert_eq!(input.supersedes, vec!["m3", "m4"]);
    }

    #[test]
    fn remember_accepts_a_single_entity_object() {
        let input = parse_remember(&args(&[
            "x",
            "--entities",
            r#"{"name":"Mom","type":"person","aliases":["mother"]}"#,
        ]))
        .unwrap();
        assert_eq!(input.entities.len(), 1);
        assert_eq!(input.entities[0].name, "Mom");
    }

    #[test]
    fn recall_parses_filters() {
        let input = parse_recall(&args(&[
            "where",
            "does",
            "Mom",
            "live",
            "--k",
            "5",
            "--kinds",
            "fact,event",
            "--history",
            "--as-of",
            "2026-01-01",
        ]))
        .unwrap();
        assert_eq!(input.query, "where does Mom live");
        assert_eq!(input.k, Some(5));
        assert_eq!(input.kinds, vec!["fact", "event"]);
        assert!(input.include_history);
        assert_eq!(input.as_of.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn missing_values_are_usage_errors() {
        assert!(matches!(
            parse_recall(&args(&["q", "--k"])),
            Err(Fail::Usage(_))
        ));
        assert!(matches!(parse_remember(&args(&[])), Err(Fail::Usage(_))));
        assert!(matches!(
            parse_remember(&args(&["x", "--nope"])),
            Err(Fail::Usage(_))
        ));
    }

    #[test]
    fn bad_entities_json_is_a_usage_error() {
        assert!(matches!(
            parse_remember(&args(&["x", "--entities", "{oops"])),
            Err(Fail::Usage(_))
        ));
    }
}
