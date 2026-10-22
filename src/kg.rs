use crate::db::Db;
use crate::error::Result;
use crate::graph::Csr;
use rusqlite::params;
use std::collections::HashMap;

pub struct Snapshot {
    pub csr: Csr,
    pub node_kind: Vec<(char, i64)>,
    pub mem_node: HashMap<i64, u32>,
    pub ent_node: HashMap<i64, u32>,
}

fn run(db: &Db, q: &str, params: &[(&str, serde_json::Value)]) -> Result<()> {
    let mut b = db.graph().cypher_builder(q);
    for (k, v) in params {
        b = b.param(k, v.clone());
    }
    b.run()?;
    Ok(())
}

pub fn project_memory(db: &Db, id: i64, kind: &str) -> Result<()> {
    run(
        db,
        "MERGE (n:Memory {id:$id}) SET n.kind=$kind",
        &[("id", format!("m:{id}").into()), ("kind", kind.into())],
    )
}

pub fn project_entity(db: &Db, id: i64, name: &str, kind: &str) -> Result<()> {
    run(
        db,
        "MERGE (n:Entity {id:$id}) SET n.name=$name, n.type=$kind",
        &[
            ("id", format!("e:{id}").into()),
            ("name", name.into()),
            ("kind", kind.into()),
        ],
    )
}

/// Project an entity using its stored name/type (so a bare mention never
/// clobbers an existing type).
pub fn project_entity_from_db(db: &Db, id: i64) -> Result<()> {
    let (name, kind): (String, String) =
        db.sql()
            .query_row("SELECT name,type FROM entities WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
    project_entity(db, id, &name, &kind)
}

/// Advance the write generation used to invalidate caches.
pub fn bump_gen(db: &Db) -> Result<()> {
    db.sql().execute(
        "INSERT INTO meta(key,value) VALUES('gen','1')
         ON CONFLICT(key) DO UPDATE SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT)",
        [],
    )?;
    Ok(())
}

pub fn project_mention(db: &Db, memory: i64, entity: i64) -> Result<()> {
    run(
        db,
        "MATCH (m:Memory {id:$m}), (e:Entity {id:$e}) MERGE (m)-[:MENTIONS]->(e)",
        &[
            ("m", format!("m:{memory}").into()),
            ("e", format!("e:{entity}").into()),
        ],
    )
}

pub fn project_similar(db: &Db, a: i64, b: i64, score: f32) -> Result<()> {
    run(db, "MATCH (a:Entity {id:$a}), (b:Entity {id:$b}) MERGE (a)-[r:SIMILAR]->(b) SET r.score=$score", &[
        ("a", format!("e:{a}").into()), ("b", format!("e:{b}").into()), ("score", f64::from(score).into()),
    ])
}

pub fn project_relation(db: &Db, id: i64, src: i64, dst: i64, predicate: &str) -> Result<()> {
    run(db, "MATCH (a:Entity {id:$a}), (b:Entity {id:$b}) MERGE (a)-[r:REL {rid:$rid}]->(b) SET r.predicate=$predicate", &[
        ("a", format!("e:{src}").into()), ("b", format!("e:{dst}").into()),
        ("rid", id.into()), ("predicate", predicate.into()),
    ])
}

pub fn remove_memory(db: &Db, id: i64) -> Result<()> {
    run(
        db,
        "MATCH (n:Memory {id:$id}) DETACH DELETE n",
        &[("id", format!("m:{id}").into())],
    )
}

pub fn remove_entity(db: &Db, id: i64) -> Result<()> {
    run(
        db,
        "MATCH (n:Entity {id:$id}) DETACH DELETE n",
        &[("id", format!("e:{id}").into())],
    )
}

/// Delete one REL edge by its relation id, leaving both entities in place.
pub fn remove_relation(db: &Db, id: i64) -> Result<()> {
    run(
        db,
        "MATCH (a)-[r:REL {rid:$rid}]->(b) DELETE r",
        &[("rid", id.into())],
    )
}

/// Re-project the edges and the entity node owned by `entity` after a merge:
/// the surviving node keeps its mentions and the relations that were moved
/// onto it. Nothing else in the projection is touched.
pub fn reproject_entity(db: &Db, entity: i64) -> Result<()> {
    project_entity_from_db(db, entity)?;
    let mentions: Vec<i64> = {
        let mut st = db
            .sql()
            .prepare("SELECT memory_id FROM mentions WHERE entity_id=?1")?;
        let rows = st.query_map([entity], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for memory in mentions {
        project_mention(db, memory, entity)?;
    }
    let similar: Vec<(i64, i64, f32)> = {
        let mut st = db
            .sql()
            .prepare("SELECT a,b,score FROM similar WHERE a=?1 OR b=?1")?;
        let rows = st.query_map([entity], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (a, b, score) in similar {
        project_similar(db, a, b, score)?;
    }
    let relations: Vec<(i64, i64, i64, String)> = {
        let mut st = db.sql().prepare(
            "SELECT r.id,r.src,r.dst,p.canonical FROM relations r
             JOIN predicates p ON p.id=r.predicate_id
             WHERE r.src=?1 OR r.dst=?1",
        )?;
        let rows = st.query_map([entity], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (id, src, dst, predicate) in relations {
        project_relation(db, id, src, dst, &predicate)?;
    }
    Ok(())
}

pub fn rebuild_all(db: &Db) -> Result<()> {
    db.graph().cypher("MATCH (n) DETACH DELETE n")?;
    {
        let mut st = db.sql().prepare("SELECT id,kind FROM memories")?;
        let rows: Vec<(i64, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, kind) in rows {
            project_memory(db, id, &kind)?;
        }
    }
    {
        let mut st = db.sql().prepare("SELECT id,name,type FROM entities")?;
        let rows: Vec<(i64, String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, name, kind) in rows {
            project_entity(db, id, &name, &kind)?;
        }
    }
    {
        let mut st = db
            .sql()
            .prepare("SELECT memory_id,entity_id FROM mentions")?;
        let rows: Vec<(i64, i64)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (m, e) in rows {
            project_mention(db, m, e)?;
        }
    }
    {
        let mut st = db.sql().prepare("SELECT a,b,score FROM similar")?;
        let rows: Vec<(i64, i64, f32)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (a, b, s) in rows {
            project_similar(db, a, b, s)?;
        }
    }
    {
        let mut st = db.sql().prepare("SELECT r.id,r.src,r.dst,p.canonical FROM relations r JOIN predicates p ON p.id=r.predicate_id")?;
        let rows: Vec<(i64, i64, i64, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, src, dst, p) in rows {
            project_relation(db, id, src, dst, &p)?;
        }
    }
    Ok(())
}

pub fn snapshot(db: &Db, as_of: Option<i64>, include_ended: bool) -> Result<Snapshot> {
    let mut nodes = Vec::new();
    let mut mem_node = HashMap::new();
    let mut ent_node = HashMap::new();
    {
        // Forgotten memories (expired without a superseding memory) are not
        // part of the graph. Superseded memories are history and stay only
        // when the caller asks for ended nodes.
        let sql = if include_ended {
            "SELECT id FROM memories WHERE expired_at IS NULL OR superseded_by IS NOT NULL ORDER BY id"
        } else {
            "SELECT id FROM memories WHERE expired_at IS NULL ORDER BY id"
        };
        let mut st = db.sql().prepare(sql)?;
        for row in st.query_map([], |r| r.get::<_, i64>(0))? {
            let id = row?;
            mem_node.insert(id, nodes.len() as u32);
            nodes.push(('m', id));
        }
    }
    {
        let mut st = db.sql().prepare("SELECT id FROM entities ORDER BY id")?;
        for row in st.query_map([], |r| r.get::<_, i64>(0))? {
            let id = row?;
            ent_node.insert(id, nodes.len() as u32);
            nodes.push(('e', id));
        }
    }
    let mut edges = Vec::new();
    let mut st = db
        .sql()
        .prepare("SELECT memory_id,entity_id FROM mentions")?;
    for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (m, e) = row?;
        if let (Some(&a), Some(&b)) = (mem_node.get(&m), ent_node.get(&e)) {
            edges.push((a, b, 1.0));
        }
    }
    if let Some(at) = as_of {
        let mut st = db.sql().prepare(
            "SELECT src,dst,evidence_count FROM relations
             WHERE valid_from<=?1 AND (valid_to IS NULL OR valid_to>?1) AND expired_at IS NULL",
        )?;
        let mut rows = st.query(params![at])?;
        while let Some(r) = rows.next()? {
            let (s, d, c): (i64, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?);
            if let (Some(&a), Some(&b)) = (ent_node.get(&s), ent_node.get(&d)) {
                edges.push((a, b, (1.0 + c as f64).ln().max(0.5) as f32));
            }
        }
    } else if include_ended {
        let mut st = db
            .sql()
            .prepare("SELECT src,dst,evidence_count FROM relations")?;
        let mut rows = st.query([])?;
        while let Some(r) = rows.next()? {
            let (s, d, c): (i64, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?);
            if let (Some(&a), Some(&b)) = (ent_node.get(&s), ent_node.get(&d)) {
                edges.push((a, b, (1.0 + c as f64).ln().max(0.5) as f32));
            }
        }
    } else {
        // No clock: "currently valid" is an SQL predicate, not a timestamp.
        let mut st = db.sql().prepare(
            "SELECT src,dst,evidence_count FROM relations
             WHERE valid_to IS NULL AND expired_at IS NULL",
        )?;
        let mut rows = st.query([])?;
        while let Some(r) = rows.next()? {
            let (s, d, c): (i64, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?);
            if let (Some(&a), Some(&b)) = (ent_node.get(&s), ent_node.get(&d)) {
                edges.push((a, b, (1.0 + c as f64).ln().max(0.5) as f32));
            }
        }
    }
    let mut st = db.sql().prepare("SELECT a,b,score FROM similar")?;
    for row in st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, f32>(2)?,
        ))
    })? {
        let (a, b, s) = row?;
        if let (Some(&u), Some(&v)) = (ent_node.get(&a), ent_node.get(&b)) {
            edges.push((u, v, s));
        }
    }
    Ok(Snapshot {
        csr: Csr::from_edges(nodes.len(), &edges),
        node_kind: nodes,
        mem_node,
        ent_node,
    })
}

pub fn communities(db: &Db) -> Result<HashMap<i64, i64>> {
    let result = db.graph().cypher("RETURN louvain(1.0)")?;
    let mut out = HashMap::new();
    for row in &result {
        // graphqlite returns algorithm results as one row whose `column_0` is
        // an array of {node_id, user_id, community} objects.
        if let Some(graphqlite::Value::Array(items)) = row.get_value("column_0") {
            for item in items {
                let graphqlite::Value::Object(fields) = item else {
                    continue;
                };
                let user = match fields.get("user_id") {
                    Some(graphqlite::Value::String(s)) => s.clone(),
                    Some(graphqlite::Value::Integer(i)) => i.to_string(),
                    _ => continue,
                };
                let community = match fields.get("community") {
                    Some(graphqlite::Value::Integer(i)) => *i,
                    Some(graphqlite::Value::Float(f)) => *f as i64,
                    _ => continue,
                };
                if let Some(id) = user.strip_prefix("e:").and_then(|x| x.parse().ok()) {
                    out.insert(id, community);
                }
            }
        } else if let Ok(user) = row.get::<String>("user_id") {
            if let Some(id) = user.strip_prefix("e:").and_then(|x| x.parse().ok()) {
                if let Ok(c) = row.get::<i64>("community") {
                    out.insert(id, c);
                }
            }
        }
    }
    let now = crate::temporal::Clock::system().now;
    db.sql().execute("DELETE FROM communities", [])?;
    for (id, community) in &out {
        db.sql().execute(
            "INSERT OR REPLACE INTO communities(entity_id,community,computed_at) VALUES (?1,?2,?3)",
            params![id, community, now],
        )?;
    }
    db.sql().execute(
        "INSERT OR REPLACE INTO meta(key,value)
         VALUES('communities_gen', COALESCE((SELECT value FROM meta WHERE key='gen'),'0'))",
        [],
    )?;
    Ok(out)
}
