//! JSON Lines export/import: backup, and moving a database to another
//! embedding model. Each line is a `RememberInput`-compatible object (extra
//! `id` / `expired` fields are ignored by `remember`), so a line can be fed
//! straight back to `neocortex remember -`.
//!
//! Ids are database-local, so import replays rows through `Engine::remember`
//! in file order and translates `supersedes` to the new ids. Superseded rows
//! travel with the export (`expired: true`) so the supersede chain survives;
//! forgotten rows are left out unless `--include-forgotten`.

use crate::engine::{parse_id, Engine, EntityIn, RelationIn, RememberInput};
use crate::error::Result;
use crate::text;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// One memory as exported. Field names match `RememberInput`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportRecord {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<ExportEntity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<ExportRelation>,
    /// `m<id>` in the exporting database.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Old ids this memory replaced (`m<id>` in the exporting database).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<String>,
    /// Retired: superseded, or forgotten when `--include-forgotten` was used.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expired: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportEntity {
    pub name: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExportRelation {
    pub source: String,
    pub predicate: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ImportReport {
    pub imported: usize,
    /// Rows replayed as history because a later row superseded them.
    pub superseded: usize,
    /// Rows retired after replay (only present with `--include-forgotten`).
    pub forgotten: usize,
}

/// `YYYY-MM-DDTHH:MM:SSZ`, UTC. The temporal parser reads this back exactly.
pub fn fmt_iso(ts: i64) -> String {
    const DAY: i64 = 86_400;
    let days = ts.div_euclid(DAY);
    let secs = ts.rem_euclid(DAY);
    let (y, m, d) = crate::temporal::civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// Every memory in id order. Forgotten rows are skipped unless asked for;
/// superseded rows are always kept so the history survives.
pub fn export_records(engine: &Engine, include_forgotten: bool) -> Result<Vec<ExportRecord>> {
    let filter = if include_forgotten {
        ""
    } else {
        "WHERE expired_at IS NULL OR superseded_by IS NOT NULL"
    };
    let sql = format!(
        "SELECT id, content, kind, tags, importance, scope, source, session, \
         valid_from, valid_to, expired_at, superseded_by FROM memories {filter} ORDER BY id"
    );
    let mut st = engine.db.sql().prepare(&sql)?;
    let base: Vec<BaseRow> = st
        .query_map([], |r| {
            Ok(BaseRow {
                id: r.get(0)?,
                content: r.get(1)?,
                kind: r.get(2)?,
                tags: r.get(3)?,
                importance: r.get(4)?,
                scope: r.get(5)?,
                source: r.get(6)?,
                session: r.get(7)?,
                valid_from: r.get(8)?,
                valid_to: r.get(9)?,
                expired: r.get::<_, Option<i64>>(10)?.is_some(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::with_capacity(base.len());
    for row in base {
        out.push(ExportRecord {
            tags: row.tags.split_whitespace().map(str::to_string).collect(),
            entities: entities_of(engine, row.id)?,
            relations: relations_of(engine, row.id)?,
            supersedes: superseded_ids(engine, row.id)?,
            id: Some(format!("m{}", row.id)),
            content: row.content,
            kind: Some(row.kind),
            importance: Some(row.importance),
            scope: Some(row.scope).filter(|s| !s.is_empty()),
            source: Some(row.source).filter(|s| !s.is_empty()),
            session: Some(row.session).filter(|s| !s.is_empty()),
            valid_from: Some(fmt_iso(row.valid_from)),
            valid_to: row.valid_to.map(fmt_iso),
            expired: row.expired,
        });
    }
    Ok(out)
}

struct BaseRow {
    id: i64,
    content: String,
    kind: String,
    tags: String,
    importance: f32,
    scope: String,
    source: String,
    session: String,
    valid_from: i64,
    valid_to: Option<i64>,
    expired: bool,
}

fn entities_of(engine: &Engine, memory: i64) -> Result<Vec<ExportEntity>> {
    let mut st = engine.db.sql().prepare(
        "SELECT e.id, e.name, e.type, e.description FROM mentions m
         JOIN entities e ON e.id = m.entity_id WHERE m.memory_id = ?1 ORDER BY e.id",
    )?;
    let rows: Vec<(i64, String, String, String)> = st
        .query_map([memory], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, name, kind, description) in rows {
        let own = text::norm_name(&name);
        let mut aliases: Vec<String> = engine
            .db
            .sql()
            .prepare(
                "SELECT alias_norm FROM entity_aliases WHERE entity_id = ?1 ORDER BY alias_norm",
            )?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        aliases.retain(|a| *a != own);
        out.push(ExportEntity {
            name,
            kind: Some(kind).filter(|s| !s.is_empty()),
            description: Some(description).filter(|s| !s.is_empty()),
            aliases,
        });
    }
    Ok(out)
}

fn relations_of(engine: &Engine, memory: i64) -> Result<Vec<ExportRelation>> {
    let mut st = engine.db.sql().prepare(
        "SELECT s.name, p.canonical, d.name, r.fact, p.exclusive, r.valid_from, r.valid_to, r.polarity
         FROM relation_evidence ev
         JOIN relations r ON r.id = ev.relation_id
         JOIN entities s ON s.id = r.src
         JOIN entities d ON d.id = r.dst
         JOIN predicates p ON p.id = r.predicate_id
         WHERE ev.memory_id = ?1 ORDER BY r.id",
    )?;
    let rows = st.query_map([memory], |r| {
        Ok(ExportRelation {
            source: r.get(0)?,
            predicate: r.get(1)?,
            target: r.get(2)?,
            fact: Some(r.get(3)?),
            exclusive: Some(r.get::<_, i64>(4)? != 0),
            negated: Some(r.get::<_, i64>(7)? == 0),
            valid_from: Some(fmt_iso(r.get(5)?)),
            valid_to: r.get::<_, Option<i64>>(6)?.map(fmt_iso),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn superseded_ids(engine: &Engine, memory: i64) -> Result<Vec<String>> {
    let mut st = engine
        .db
        .sql()
        .prepare("SELECT id FROM memories WHERE superseded_by = ?1 ORDER BY id")?;
    let ids = st
        .query_map([memory], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ids.into_iter().map(|id| format!("m{id}")).collect())
}

/// Write one JSON object per line; returns the number of memories written.
pub fn write_jsonl<W: Write>(w: &mut W, records: &[ExportRecord]) -> Result<usize> {
    for record in records {
        serde_json::to_writer(&mut *w, record)?;
        w.write_all(b"\n")?;
    }
    Ok(records.len())
}

/// Export to `out` (or stdout). Returns the number of memories written.
pub fn export(engine: &Engine, out: Option<&Path>, include_forgotten: bool) -> Result<usize> {
    let records = export_records(engine, include_forgotten)?;
    match out {
        Some(path) => {
            let file = create_private(path)?;
            let mut writer = std::io::BufWriter::new(file);
            let n = write_jsonl(&mut writer, &records)?;
            writer.flush()?;
            Ok(n)
        }
        None => {
            let stdout = std::io::stdout();
            let mut writer = stdout.lock();
            let n = write_jsonl(&mut writer, &records)?;
            writer.flush()?;
            Ok(n)
        }
    }
}

/// An export file is a full copy of the memory: 0600 from the first byte.
fn create_private(path: &Path) -> Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // mode() only applies at creation; tighten a file that already existed.
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    Ok(file)
}

/// Replay a JSON Lines export through `Engine::remember_batch`, in file order.
///
/// A record whose `supersedes` points at a row that is still pending in the
/// current chunk cannot be translated yet, so the chunk is flushed first: the
/// batch then sees exactly the ids the sequential replay would have seen.
pub fn import(engine: &Engine, path: &Path) -> Result<ImportReport> {
    let records = read_jsonl(path)?;
    let mut report = ImportReport::default();
    // (new id, replayed into an existing live row) per old id.
    let mut ids: HashMap<i64, (i64, bool)> = HashMap::new();
    let mut pending: Vec<(usize, Option<i64>, RememberInput)> = Vec::new();
    for (line, record) in &records {
        let old = record.id.as_deref().and_then(|s| parse_id(s, 'm'));
        let mut supersedes: Vec<String> = record
            .supersedes
            .iter()
            .filter_map(|s| parse_id(s, 'm'))
            .filter_map(|old| ids.get(&old).map(|(new, _)| format!("m{new}")))
            .collect();
        let unresolved = record
            .supersedes
            .iter()
            .filter_map(|s| parse_id(s, 'm'))
            .any(|old| !ids.contains_key(&old));
        if unresolved && !pending.is_empty() {
            flush(engine, &mut pending, &mut ids, &mut report)?;
            supersedes = record
                .supersedes
                .iter()
                .filter_map(|s| parse_id(s, 'm'))
                .filter_map(|old| ids.get(&old).map(|(new, _)| format!("m{new}")))
                .collect();
        }
        pending.push((*line, old, to_input(record, supersedes)));
        if pending.len() >= crate::remember::BATCH_SIZE {
            flush(engine, &mut pending, &mut ids, &mut report)?;
        }
    }
    flush(engine, &mut pending, &mut ids, &mut report)?;
    // Superseded rows come back as history through the replay above. Rows
    // that are still live but were marked expired were forgotten.
    for (_, record) in &records {
        if !record.expired {
            continue;
        }
        let Some((new, duplicate)) = record
            .id
            .as_deref()
            .and_then(|s| parse_id(s, 'm'))
            .and_then(|old| ids.get(&old).copied())
        else {
            continue;
        };
        // A duplicate hit means this row is an existing live memory: never
        // retire it (restoring an --include-forgotten export must not forget).
        if duplicate {
            continue;
        }
        let state: (Option<i64>, Option<i64>) = engine.db.sql().query_row(
            "SELECT expired_at, superseded_by FROM memories WHERE id = ?1",
            [new],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if state.1.is_some() {
            report.superseded += 1;
        } else {
            if state.0.is_none() {
                engine.forget(&format!("m{new}"), false)?;
            }
            report.forgotten += 1;
        }
    }
    Ok(report)
}

fn flush(
    engine: &Engine,
    pending: &mut Vec<(usize, Option<i64>, RememberInput)>,
    ids: &mut HashMap<i64, (i64, bool)>,
    report: &mut ImportReport,
) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let batch: Vec<(usize, Option<i64>)> = pending.iter().map(|(l, o, _)| (*l, *o)).collect();
    let inputs: Vec<RememberInput> = pending.drain(..).map(|(_, _, input)| input).collect();
    for ((line, old), result) in batch.into_iter().zip(engine.remember_batch(inputs)) {
        let out = result.map_err(|e| crate::err!("line {line}: {e}"))?;
        if let Some(old) = old {
            ids.insert(old, (out.id, out.duplicate_of.is_some()));
        }
        report.imported += 1;
    }
    Ok(())
}

fn read_jsonl(path: &Path) -> Result<Vec<(usize, ExportRecord)>> {
    let file = std::fs::File::open(path)?;
    let mut out = Vec::new();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record: ExportRecord = serde_json::from_str(line)
            .map_err(|e| crate::err!("{}:{}: {e}", path.display(), i + 1))?;
        out.push((i + 1, record));
    }
    Ok(out)
}

fn to_input(record: &ExportRecord, supersedes: Vec<String>) -> RememberInput {
    RememberInput {
        content: record.content.clone(),
        kind: record.kind.clone(),
        tags: record.tags.clone(),
        importance: record.importance,
        scope: record.scope.clone(),
        source: record.source.clone(),
        session: record.session.clone(),
        valid_from: record.valid_from.clone(),
        valid_to: record.valid_to.clone(),
        entities: record
            .entities
            .iter()
            .map(|e| EntityIn {
                name: e.name.clone(),
                kind: e.kind.clone(),
                description: e.description.clone(),
                aliases: e.aliases.clone(),
                same_as: None,
            })
            .collect(),
        relations: record
            .relations
            .iter()
            .map(|r| RelationIn {
                source: r.source.clone(),
                predicate: r.predicate.clone(),
                target: r.target.clone(),
                fact: r.fact.clone(),
                exclusive: r.exclusive,
                negated: r.negated,
                valid_from: r.valid_from.clone(),
                valid_to: r.valid_to.clone(),
            })
            .collect(),
        keywords: Vec::new(),
        supersedes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_format_round_trips_through_the_temporal_parser() {
        let clock = crate::temporal::Clock {
            now: 0,
            tz_offset_secs: 0,
        };
        for ts in [0, 1_700_000_000, 1_782_000_123] {
            let text = fmt_iso(ts);
            assert_eq!(text.len(), 20, "{text}");
            assert!(text.ends_with('Z'), "{text}");
            assert_eq!(crate::temporal::parse_instant(&text, &clock), Some(ts));
        }
    }

    #[test]
    fn iso_format_uses_utc_not_the_clock_offset() {
        // 2026-10-21T15:00:00Z
        assert_eq!(fmt_iso(1_792_594_800), "2026-10-21T15:00:00Z");
    }
}
