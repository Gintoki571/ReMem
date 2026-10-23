//! CLI: hand-rolled argument parsing, one subcommand per MCP tool plus
//! `mcp`, `reindex`, `validate`, `cypher` and `import-remem`.
//!
//! Exit codes: 0 ok, 1 runtime error, 2 usage error.

use neocortex::embed::{Embedder, HashEmbedder};
use neocortex::engine::{Engine, EntityIn, RecallInput, RelationIn, RememberInput};
use neocortex::mcp::LazyEmbedder;
use neocortex::render;
use neocortex::{bench, export, import, mcp, Error};
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
    let (mut globals, start) = parse_globals(args)?;
    // --json is accepted after the command too (`recall x --json`).
    let rest: Vec<String> = args[start..]
        .iter()
        .filter(|a| {
            let json = a.as_str() == "--json";
            globals.json |= json;
            !json
        })
        .cloned()
        .collect();
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
        "export" => cmd_export(&globals, rest),
        "import" => cmd_import_jsonl(&globals, rest),
        "bench" => cmd_bench(&globals, rest),
        "eval" => cmd_eval(&globals, rest),
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

fn cap_items<T>(field: &str, items: Vec<T>) -> std::result::Result<Vec<T>, Fail> {
    if items.len() > 64 {
        Err(usage(format!("{field} exceeds the 64-item limit")))
    } else {
        Ok(items)
    }
}

fn cap_text(field: &str, value: &str, max: usize) -> std::result::Result<(), Fail> {
    if value.len() > max {
        Err(usage(format!("{field} exceeds the {max}-byte limit")))
    } else {
        Ok(())
    }
}

fn validate_remember_names(input: &RememberInput) -> std::result::Result<(), Fail> {
    for entity in &input.entities {
        for (field, value) in [("entity name", entity.name.as_str())]
            .into_iter()
            .chain(entity.kind.as_deref().map(|v| ("entity type", v)))
            .chain(entity.aliases.iter().map(|v| ("entity alias", v.as_str())))
        {
            if value.chars().count() > 200 {
                return Err(usage(format!("{field} exceeds the 200-character limit")));
            }
        }
    }
    for relation in &input.relations {
        for (field, value) in [
            ("relation source", relation.source.as_str()),
            ("relation predicate", relation.predicate.as_str()),
            ("relation target", relation.target.as_str()),
        ] {
            if value.chars().count() > 200 {
                return Err(usage(format!("{field} exceeds the 200-character limit")));
            }
        }
    }
    Ok(())
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
    Engine::open(&path, make_embedder()).map_err(runtime)
}

fn make_embedder() -> Arc<dyn Embedder> {
    match std::env::var("NEOCORTEX_EMBEDDER").as_deref() {
        Ok("hash") => Arc::new(HashEmbedder),
        _ => Arc::new(LazyEmbedder::new()),
    }
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
                input.entities =
                    cap_items("--entities", json_list::<EntityIn>(&raw, "--entities")?)?;
            }
            "--relations" => {
                let raw = value(args, &mut i, arg)?;
                input.relations =
                    cap_items("--relations", json_list::<RelationIn>(&raw, "--relations")?)?;
            }
            "--keywords" => {
                input.keywords = cap_items("--keywords", list(&value(args, &mut i, arg)?))?
            }
            "--supersedes" => {
                input.supersedes = cap_items("--supersedes", list(&value(args, &mut i, arg)?))?
            }
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
        let input: RememberInput = serde_json::from_str(&raw)
            .map_err(|e| usage(format!("invalid RememberInput JSON on stdin: {e}")))?;
        cap_text("content", &input.content, 32 * 1024)?;
        if input.entities.len() > 64 {
            return Err(usage("entities exceeds the 64-item limit"));
        }
        if input.relations.len() > 64 {
            return Err(usage("relations exceeds the 64-item limit"));
        }
        if input.tags.len() > 64 {
            return Err(usage("tags exceeds the 64-item limit"));
        }
        if input.keywords.len() > 64 {
            return Err(usage("keywords exceeds the 64-item limit"));
        }
        if input.supersedes.len() > 64 {
            return Err(usage("supersedes exceeds the 64-item limit"));
        }
        validate_remember_names(&input)?;
        return Ok(input);
    }
    if words.is_empty() {
        return Err(usage(
            "remember needs content, or `remember -` to read a JSON object from stdin",
        ));
    }
    input.content = words.join(" ");
    cap_text("content", &input.content, 32 * 1024)?;
    if input.tags.len() > 64 {
        return Err(usage("tags exceeds the 64-item limit"));
    }
    validate_remember_names(&input)?;
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
            "--k" => {
                let n = number(args, &mut i, arg)?;
                if n > 50 {
                    return Err(usage("--k exceeds the 50 limit"));
                }
                input.k = Some(n);
            }
            "--budget" => {
                let n = number(args, &mut i, arg)?;
                if n > 20_000 {
                    return Err(usage("--budget exceeds the 20000 limit"));
                }
                input.budget = Some(n);
            }
            "--kinds" => input.kinds = list(&value(args, &mut i, arg)?),
            "--tags" => input.tags = cap_items("--tags", list(&value(args, &mut i, arg)?))?,
            "--scope" => input.scope = Some(value(args, &mut i, arg)?),
            "--source" => input.source = Some(value(args, &mut i, arg)?),
            "--session" => input.session = Some(value(args, &mut i, arg)?),
            "--since" => input.since = Some(value(args, &mut i, arg)?),
            "--until" => input.until = Some(value(args, &mut i, arg)?),
            "--as-of" => input.as_of = Some(value(args, &mut i, arg)?),
            "--history" => input.include_history = true,
            "--entities" => {
                input.entities = cap_items("--entities", list(&value(args, &mut i, arg)?))?
            }
            "--keywords" => {
                input.keywords = cap_items("--keywords", list(&value(args, &mut i, arg)?))?
            }
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
    cap_text("query", &input.query, 2 * 1024)?;
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
            "--limit" => {
                limit = number(args, &mut i, arg)?;
                if limit > 200 {
                    return Err(usage("--limit exceeds the 200 limit"));
                }
            }
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option {other}")));
            }
            // Unquoted multi-word names ("entity Mom Smith") are one name.
            _ => {
                name = Some(match name {
                    Some(prev) => format!("{prev} {}", args[i]),
                    None => args[i].clone(),
                })
            }
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
            "--k" => {
                k = number(args, &mut i, arg)?;
                if k > 50 {
                    return Err(usage("--k exceeds the 50 limit"));
                }
            }
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

fn cmd_export(globals: &Globals, args: &[String]) -> CmdResult {
    let mut out: Option<String> = None;
    let mut include_forgotten = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => out = Some(value(args, &mut i, "--out")?),
            "--include-forgotten" => include_forgotten = true,
            other => return Err(usage(format!("unknown option {other}"))),
        }
        i += 1;
    }
    let engine = open_engine(globals)?;
    let count = export::export(&engine, out.as_deref().map(Path::new), include_forgotten)
        .map_err(runtime)?;
    match out {
        Some(path) => println!("exported {count} memories to {path}"),
        // stdout carries the JSON Lines, so the status goes to stderr.
        None => eprintln!("exported {count} memories"),
    }
    Ok(())
}

fn cmd_import_jsonl(globals: &Globals, args: &[String]) -> CmdResult {
    let path = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .ok_or_else(|| usage("import needs a JSON Lines file (see `neocortex export`)"))?;
    let engine = open_engine(globals)?;
    let report = export::import(&engine, Path::new(path)).map_err(runtime)?;
    if globals.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| Fail::Run(e.to_string()))?
        );
    } else {
        println!(
            "imported {} memories ({} superseded, {} forgotten)",
            report.imported, report.superseded, report.forgotten
        );
    }
    Ok(())
}

fn cmd_bench(globals: &Globals, args: &[String]) -> CmdResult {
    let mut opts = bench::Options::default();
    let mut db: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--n" => opts.memories = number(args, &mut i, "--n")?,
            "--queries" => opts.queries = number(args, &mut i, "--queries")?,
            "--db" => db = Some(PathBuf::from(value(args, &mut i, "--db")?)),
            other => return Err(usage(format!("unknown option {other}"))),
        }
        i += 1;
    }
    let (path, temporary) = match db.or_else(|| globals.db.clone().map(PathBuf::from)) {
        Some(path) => (path, false),
        None => {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir()
                .join(format!("neocortex-bench-{}-{nanos}.db", std::process::id()));
            (path, true)
        }
    };
    eprintln!(
        "bench: {} memories, {} queries, db {}",
        opts.memories,
        opts.queries,
        path.display()
    );
    let result = bench::run(&path, make_embedder(), opts);
    if temporary {
        remove_db_files(&path);
    }
    let report = result.map_err(runtime)?;
    emit(globals, &report, || report.render())
}

fn remove_db_files(path: &Path) {
    let _ = std::fs::remove_file(path);
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

fn cmd_eval(globals: &Globals, args: &[String]) -> CmdResult {
    let path = args
        .first()
        .filter(|x| !x.starts_with("--"))
        .ok_or_else(|| usage("eval needs a fixtures.json path"))?;
    let mut ablation = false;
    let mut failures = false;
    let mut sets: Vec<String> = Vec::new();
    let mut sweep: Option<String> = None;
    let mut opts = neocortex::eval::EvalOptions::default();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--ablation" => ablation = true,
            "--failures" => failures = true,
            "--set" => {
                let item = value(args, &mut i, "--set")?;
                sets.push(item);
            }
            "--sweep" => {
                let item = value(args, &mut i, "--sweep")?;
                sweep = Some(item);
            }
            "--limit-queries" => {
                let item = value(args, &mut i, "--limit-queries")?;
                opts.limit_queries =
                    Some(item.parse().map_err(|_| {
                        usage(format!("--limit-queries needs a number, got '{item}'"))
                    })?);
            }
            "--save-db" => {
                opts.save_db = Some(PathBuf::from(value(args, &mut i, "--save-db")?));
            }
            "--load-db" => {
                opts.load_db = Some(PathBuf::from(value(args, &mut i, "--load-db")?));
            }
            other if other.starts_with("--set=") => {
                sets.push(other["--set=".len()..].to_string());
            }
            other if other.starts_with("--sweep=") => {
                sweep = Some(other["--sweep=".len()..].to_string());
            }
            other if other.starts_with("--limit-queries=") => {
                let item = &other["--limit-queries=".len()..];
                opts.limit_queries =
                    Some(item.parse().map_err(|_| {
                        usage(format!("--limit-queries needs a number, got '{item}'"))
                    })?);
            }
            other if other.starts_with("--save-db=") => {
                opts.save_db = Some(PathBuf::from(&other["--save-db=".len()..]));
            }
            other if other.starts_with("--load-db=") => {
                opts.load_db = Some(PathBuf::from(&other["--load-db=".len()..]));
            }
            other => return Err(usage(format!("unknown eval flag: {other}"))),
        }
        i += 1;
    }
    if opts.save_db.is_some() && opts.load_db.is_some() {
        return Err(usage("--save-db and --load-db are mutually exclusive"));
    }
    // eval builds its own databases; it must never open (or migrate) the
    // user's memory, so --db/$NEOCORTEX_DB are ignored here.
    let mut cfg = neocortex::engine::Config::default();
    for item in &sets {
        let (key, val) = item
            .split_once('=')
            .ok_or_else(|| usage(format!("--set needs key=value, got '{item}'")))?;
        neocortex::eval::set_config(&mut cfg, key, val).map_err(runtime)?;
    }
    let emb = make_embedder();
    let path = Path::new(path);
    if let Some(spec) = sweep {
        let (key, values) = spec
            .split_once('=')
            .ok_or_else(|| usage(format!("--sweep needs key=v1,v2, got '{spec}'")))?;
        let mut variants = Vec::new();
        for val in list(values) {
            let mut variant = cfg.clone();
            neocortex::eval::set_config(&mut variant, key, &val).map_err(runtime)?;
            variants.push((format!("{key}={val}"), variant));
        }
        if variants.is_empty() {
            return Err(usage("--sweep needs at least one value"));
        }
        let rows = neocortex::eval::sweep_with(path, emb, cfg, variants, &opts).map_err(runtime)?;
        if globals.json {
            let value: Vec<serde_json::Value> = rows
                .iter()
                .map(|(name, report)| serde_json::json!({"variant":name,"report":report}))
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(|e| Fail::Run(e.to_string()))?
            );
        } else {
            print!("{}", neocortex::eval::sweep_table(&rows));
        }
        return Ok(());
    }
    if ablation {
        let mut variants = vec![("full".to_string(), cfg.clone())];
        variants.extend(neocortex::eval::ablation_variants(&cfg));
        let rows = neocortex::eval::sweep_with(path, emb, cfg, variants, &opts).map_err(runtime)?;
        if globals.json {
            let mut value =
                serde_json::to_value(&rows[0].1).map_err(|e| Fail::Run(e.to_string()))?;
            let ablation: Vec<serde_json::Value> = rows[1..]
                .iter()
                .map(|(name, report)| {
                    serde_json::json!({"name":name,"recall":report.recall,"mrr":report.mrr})
                })
                .collect();
            value["ablation"] = serde_json::Value::Array(ablation);
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(|e| Fail::Run(e.to_string()))?
            );
        } else {
            print!("{}", neocortex::eval::render_report(&rows[0].1, failures));
            for (name, result) in &rows[1..] {
                println!(
                    "ablation {name}: recall {:.3}, mrr {:.3}",
                    result.recall, result.mrr
                );
            }
        }
        return Ok(());
    }
    let report = neocortex::eval::run_eval(path, emb, cfg, &opts).map_err(runtime)?;
    if globals.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| Fail::Run(e.to_string()))?
        );
    } else {
        print!("{}", neocortex::eval::render_report(&report, failures));
    }
    Ok(())
}

fn print_help() {
    println!(
        "neocortex {} — long-term memory for AI agents

Usage: neocortex [--db PATH] [--json] <command> [args]

Commands:
  remember <content...>    store a memory (max 32 KiB); `remember -` reads a JSON object from stdin
  recall <query...>        retrieve memories, facts and entities
  entity <name|eID>        profile and timeline of one entity (--limit N, default 20)
  invalidate <mID|rID>     end a memory or relation, keeping history (--at DATE, default now)
  forget <mID>             retire a memory (--hard deletes it)
  merge <keep> <merge>     merge one entity into another
  alias <entity> <alias>   add a name to an entity
  overview [query...]      communities of the entity graph
  stats                    counts, database size and embedder
  reindex                  rebuild the graph projection
  validate                 report integrity problems (exit 1 if any)
  cypher <query>           run read-only Cypher
  import-remem <path>      import a ReMem v3 SQLite file
  export [--out FILE]      write memories as JSON Lines (--include-forgotten)
  import <file.jsonl>      replay a JSON Lines export through remember
  bench                    synthetic remember/recall latency report
                           (--n 10000 --queries 200 --db PATH)
  eval <fixtures.json>     evaluate retrieval quality (--ablation --failures
                           --set key=value --sweep key=v1,v2
                           --limit-queries N --save-db PATH --load-db PATH)
  mcp                      run the MCP stdio server
  help                     this text
  --version                print the version

Common flags: --db PATH (default $NEOCORTEX_DB, before the command), --json (machine-readable output, anywhere).
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
