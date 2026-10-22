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
        let mut st = db.sql().prepare("SELECT id FROM memories ORDER BY id")?;
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
    let now = as_of.unwrap_or_else(|| crate::temporal::Clock::system().now);
    let sql = if include_ended {
        "SELECT src,dst,evidence_count FROM relations"
    } else {
        "SELECT src,dst,evidence_count FROM relations WHERE valid_from<=?1 AND (valid_to IS NULL OR valid_to>?1) AND expired_at IS NULL"
    };
    let mut st = db.sql().prepare(sql)?;
    let iter = if include_ended {
        st.query([])?
    } else {
        st.query(params![now])?
    };
    let mut rows = iter;
    while let Some(r) = rows.next()? {
        let (s, d, c): (i64, i64, i64) = (r.get(0)?, r.get(1)?, r.get(2)?);
        if let (Some(&a), Some(&b)) = (ent_node.get(&s), ent_node.get(&d)) {
            edges.push((a, b, (1.0 + (c as f64).ln()).max(0.5) as f32));
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
    for row in result {
        if let Ok(user) = row.get::<String>("user_id") {
            if let Some(id) = user.strip_prefix("e:").and_then(|x| x.parse().ok()) {
                if let Ok(c) = row.get::<i64>("community") {
                    out.insert(id, c);
                }
            }
        }
    }
    Ok(out)
}
