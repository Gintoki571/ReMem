use crate::{
    db::{vec_blob, Db},
    embed::DIMS,
    engine::{Config, EntityIn},
    error::Result,
    text,
};
use rusqlite::params;

#[derive(Clone, Debug)]
pub struct Resolved {
    pub id: i64,
    pub status: String,
    pub similar: Vec<(i64, String, f32)>,
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn find_exact(db: &Db, norm: &str, kind: &str) -> Result<Option<i64>> {
    let mut st = db.sql().prepare(
        "SELECT e.id FROM entity_aliases a JOIN entities e ON e.id=a.entity_id
         WHERE a.alias_norm=?1 AND (?2='' OR e.type='' OR e.type=?2)
         ORDER BY e.mention_count DESC LIMIT 1",
    )?;
    Ok(st.query_row(params![norm, kind], |r| r.get(0)).optional()?)
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
    let forced = item
        .same_as
        .as_deref()
        .and_then(|s| crate::engine::parse_id(s, 'e'));
    if let Some(id) = forced {
        add_aliases(db, id, item, "agent")?;
        return Ok(Resolved {
            id,
            status: "same_as".into(),
            similar: Vec::new(),
        });
    }
    if let Some(id) = find_exact(db, &norm, kind)? {
        add_aliases(db, id, item, "agent")?;
        return Ok(Resolved {
            id,
            status: if norm == text::norm_name(&item.name) {
                "matched"
            } else {
                "alias"
            }
            .into(),
            similar: Vec::new(),
        });
    }
    let mut candidates = Vec::new();
    let mut st = db
        .sql()
        .prepare("SELECT id,norm,name,type,description FROM entities")?;
    for row in st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })? {
        let (id, en, name, et, desc) = row?;
        if !kind.is_empty() && !et.is_empty() && et != kind {
            continue;
        }
        let lexical = text::jaro_winkler(&norm, &en) >= 0.93
            && norm.chars().count() >= 5
            && en.chars().count() >= 5
            && !different_digits(&norm, &en)
            && !different_tokens(&norm, &en);
        let acronym = text::acronym(&norm).as_deref() == Some(&en.to_uppercase())
            || text::acronym(&en).as_deref() == Some(&norm.to_uppercase());
        let score = entity_embedding(db, id, embedding)?;
        if lexical || (acronym && score >= 0.6) {
            candidates.push((id, name, lexical, score));
        } else if score >= cfg.sim_edge {
            candidates.push((id, name, false, score));
        }
        let _ = desc;
    }
    candidates.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    if let Some((id, _, lexical, score)) = candidates.first() {
        if *lexical || text::acronym(&norm).is_some() {
            add_aliases(db, *id, item, if *lexical { "lexical" } else { "acronym" })?;
            return Ok(Resolved {
                id: *id,
                status: if *lexical { "lexical" } else { "acronym" }.into(),
                similar: Vec::new(),
            });
        }
        let _ = score;
    }
    db.sql().execute("INSERT INTO entities(name,norm,type,description,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?5)", params![item.name,norm,kind,item.description.as_deref().unwrap_or(""),now])?;
    let id = db.sql().last_insert_rowid();
    db.sql().execute(
        "INSERT INTO entity_aliases(alias_norm,entity_id,origin) VALUES (?1,?2,'name')",
        params![text::norm_name(&item.name), id],
    )?;
    for alias in &item.aliases {
        db.sql().execute("INSERT OR IGNORE INTO entity_aliases(alias_norm,entity_id,origin) VALUES (?1,?2,'agent')", params![text::norm_name(alias),id])?;
    }
    db.sql().execute(
        "INSERT INTO entities_fts(rowid,name,aliases,description) VALUES (?1,?2,?3,?4)",
        params![
            id,
            item.name,
            item.aliases.join(" "),
            item.description.as_deref().unwrap_or("")
        ],
    )?;
    db.sql().execute(
        "INSERT INTO entities_vec(rowid,embedding) VALUES (?1,?2)",
        params![id, vec_blob(embedding)],
    )?;
    let similar = candidates
        .into_iter()
        .filter(|x| x.3 >= cfg.sim_edge)
        .take(3)
        .map(|(i, n, _, s)| (i, n, s))
        .collect();
    Ok(Resolved {
        id,
        status: "new".into(),
        similar,
    })
}

fn entity_embedding(db: &Db, id: i64, mention: &[f32]) -> Result<f32> {
    let blob: Vec<u8> = db
        .sql()
        .query_row(
            "SELECT embedding FROM entities_vec WHERE rowid=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap_or_default();
    if blob.len() != DIMS * 4 {
        return Ok(0.0);
    }
    let v: Vec<f32> = blob
        .chunks(4)
        .filter(|x| x.len() == 4)
        .map(|x| f32::from_le_bytes([x[0], x[1], x[2], x[3]]))
        .collect();
    Ok(cosine(mention, &v))
}

fn add_aliases(db: &Db, id: i64, item: &EntityIn, origin: &str) -> Result<()> {
    for alias in std::iter::once(&item.name).chain(item.aliases.iter()) {
        db.sql().execute(
            "INSERT OR IGNORE INTO entity_aliases(alias_norm,entity_id,origin) VALUES (?1,?2,?3)",
            params![text::norm_name(alias), id, origin],
        )?;
    }
    db.sql().execute(
        "UPDATE entities SET mention_count=mention_count+1, updated_at=updated_at WHERE id=?1",
        [id],
    )?;
    Ok(())
}

fn different_digits(a: &str, b: &str) -> bool {
    let da: String = a.chars().filter(|c| c.is_ascii_digit()).collect();
    let db: String = b.chars().filter(|c| c.is_ascii_digit()).collect();
    !da.is_empty() && da != db
}
fn different_tokens(a: &str, b: &str) -> bool {
    let aa: std::collections::HashSet<_> = text::tokens(a)
        .into_iter()
        .filter(|x| !text::is_stopword(x))
        .collect();
    let bb: std::collections::HashSet<_> = text::tokens(b)
        .into_iter()
        .filter(|x| !text::is_stopword(x))
        .collect();
    aa.symmetric_difference(&bb).next().is_some()
}

pub fn resolve_predicate(
    db: &Db,
    name: &str,
    embedding: &[f32],
    exclusive: bool,
) -> Result<(i64, String)> {
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
        return Ok((id, norm));
    }
    db.sql().execute(
        "INSERT INTO predicates(canonical,exclusive) VALUES (?1,?2)",
        params![norm, exclusive as i64],
    )?;
    let id = db.sql().last_insert_rowid();
    db.sql().execute(
        "INSERT INTO predicate_aliases(alias_norm,predicate_id) VALUES (?1,?2)",
        params![text::norm_predicate(name), id],
    )?;
    db.sql().execute(
        "INSERT INTO predicates_vec(rowid,embedding) VALUES (?1,?2)",
        params![id, vec_blob(embedding)],
    )?;
    Ok((id, norm))
}

trait OptionalRow<T> {
    fn optional(self) -> rusqlite::Result<Option<T>>;
}
impl<T> OptionalRow<T> for rusqlite::Result<T> {
    fn optional(self) -> rusqlite::Result<Option<T>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
