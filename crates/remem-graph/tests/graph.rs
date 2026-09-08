use remem_graph::{Graph, BELONGS_TO_AGENT, BELONGS_TO_SESSION, MEMORY_LABEL};
use remem_types::MemoryKind;
use serde_json::json;

fn graph() -> Graph {
    Graph::open_in_memory().expect("open in-memory graph")
}

fn sorted(mut v: Vec<(String, String)>) -> Vec<(String, String)> {
    v.sort();
    v
}

#[test]
fn link_then_neighbors() {
    let g = graph();
    g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    g.upsert_memory("m2", MemoryKind::Decision).unwrap();
    g.link("m1", "m2", "RELATES_TO").unwrap();

    assert_eq!(
        sorted(g.neighbors("m1").unwrap()),
        vec![("m2".to_string(), "RELATES_TO".to_string())]
    );
    // undirected read: the other end sees the edge too
    assert_eq!(
        sorted(g.neighbors("m2").unwrap()).remove(0).0,
        "m1".to_string()
    );
}

#[test]
fn neighbors_distinguish_direction() {
    let g = graph();
    for id in ["a", "b"] {
        g.upsert_memory(id, MemoryKind::Note).unwrap();
    }
    g.link("a", "b", "CAUSES").unwrap();
    let out = g.neighbors_detail("a").unwrap();
    let inn = g.neighbors_detail("b").unwrap();
    assert_eq!(out.len(), 1);
    assert!(out[0].outgoing, "a -> b must read as outgoing");
    assert_eq!(out[0].rel, "CAUSES");
    assert_eq!(inn.len(), 1);
    assert!(!inn[0].outgoing, "b's edge to a is incoming");
}

#[test]
fn custom_and_default_edge_types() {
    let g = graph();
    g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    g.upsert_memory("m2", MemoryKind::Fact).unwrap();
    g.upsert_memory("m3", MemoryKind::Fact).unwrap();
    g.relates_to("m1", "m2").unwrap();
    g.link("m1", "m3", "SUPERSEDES").unwrap();
    assert_eq!(
        sorted(g.neighbors("m1").unwrap()),
        vec![
            ("m2".to_string(), "RELATES_TO".to_string()),
            ("m3".to_string(), "SUPERSEDES".to_string()),
        ]
    );
}

#[test]
fn linking_is_idempotent() {
    let g = graph();
    g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    g.upsert_memory("m2", MemoryKind::Fact).unwrap();
    g.link("m1", "m2", "RELATES_TO").unwrap();
    g.link("m1", "m2", "RELATES_TO").unwrap();
    assert_eq!(g.neighbors("m1").unwrap().len(), 1);
    assert_eq!(g.stats().unwrap()["edges"], json!(1));
}

#[test]
fn link_rejects_unknown_nodes() {
    let g = graph();
    g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    let err = g.link("m1", "nope", "RELATES_TO").unwrap_err();
    assert!(err.to_string().contains("nope"), "{err}");
}

#[test]
fn upsert_memory_updates_kind() {
    let g = graph();
    g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    g.upsert_memory("m1", MemoryKind::Mistake).unwrap();
    let rows = g
        .cypher(
            "MATCH (n:Memory {mid: $id}) RETURN n.kind AS kind",
            &json!({"id": "m1"}),
        )
        .unwrap();
    assert_eq!(rows, json!([{"kind": "mistake"}]));
    assert_eq!(g.stats().unwrap()["nodes"], json!(1));
}

#[test]
fn forget_removes_node_and_edges() {
    let g = graph();
    g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    g.upsert_memory("m2", MemoryKind::Fact).unwrap();
    g.link("m1", "m2", "RELATES_TO").unwrap();
    g.forget("m1").unwrap();
    assert!(g.neighbors("m2").unwrap().is_empty());
    assert_eq!(g.stats().unwrap()["nodes"], json!(1));
}

#[test]
fn hubs_are_shared_by_memories() {
    let g = graph();
    let mut first = remem_types::MemoryItem::new(MemoryKind::Fact, "one".into());
    first.agent_id = "agent-a".into();
    first.session_id = "sess-1".into();
    let mut second = remem_types::MemoryItem::new(MemoryKind::Decision, "two".into());
    second.agent_id = "agent-a".into();
    second.session_id = "sess-2".into();

    g.attach(&first).unwrap();
    g.attach(&second).unwrap();

    // one Agent hub shared by both memories
    let hubs = g
        .cypher("MATCH (a:Agent) RETURN a.aid AS aid", &json!({}))
        .unwrap();
    assert_eq!(hubs, json!([{"aid": "agent-a"}]));
    let owned = g
        .cypher(
            "MATCH (m:Memory)-[:BELONGS_TO_AGENT]->(a:Agent {aid: $aid}) \
             RETURN m.mid AS mid ORDER BY mid",
            &json!({"aid": "agent-a"}),
        )
        .unwrap();
    let mut mids: Vec<String> = owned
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["mid"].as_str().unwrap().to_string())
        .collect();
    mids.sort();
    let mut want = vec![first.id.clone(), second.id.clone()];
    want.sort();
    assert_eq!(mids, want);

    // one Session hub per session, reached through the agent hub
    let sessions = g
        .cypher(
            "MATCH (m:Memory {mid: $id})-[:BELONGS_TO_SESSION]->(s:Session) \
             RETURN s.sid AS sid",
            &json!({"id": first.id}),
        )
        .unwrap();
    assert_eq!(sessions, json!([{"sid": "sess-1"}]));
    // memories under one agent span both sessions (hub-to-hub has no direct
    // edge, so the hop goes through the memory)
    let via_hub = g
        .cypher(
            "MATCH (s:Session)<-[:BELONGS_TO_SESSION]-(m:Memory)-[:BELONGS_TO_AGENT]->(a:Agent) \
             RETURN a.aid AS aid, s.sid AS sid",
            &json!({}),
        )
        .unwrap();
    let rows = via_hub.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r["aid"] == json!("agent-a")));
}

#[test]
fn neighbors_skip_hub_nodes() {
    let g = graph();
    let mut item = remem_types::MemoryItem::new(MemoryKind::Fact, "one".into());
    item.agent_id = "agent-a".into();
    item.session_id = "sess-1".into();
    g.attach(&item).unwrap();
    g.upsert_memory("m2", MemoryKind::Fact).unwrap();
    g.relates_to(&item.id, "m2").unwrap();

    // recall expansion wants memory ids only; a hub id is not one
    assert_eq!(
        g.neighbors(&item.id).unwrap(),
        vec![("m2".to_string(), "RELATES_TO".to_string())]
    );
    // the hubs are still visible through the detail view
    let detail = g.neighbors_detail(&item.id).unwrap();
    assert_eq!(detail.len(), 3);
    assert!(detail.iter().any(|n| n.labels == vec!["Agent".to_string()]
        && n.rel == remem_graph::BELONGS_TO_AGENT
        && n.outgoing));
    assert!(detail
        .iter()
        .any(|n| n.labels == vec!["Session".to_string()]));
}

#[test]
fn hub_id_cannot_collide_with_a_memory_id() {
    let g = graph();
    // graphqlite keeps one id space for all labels, so an agent id equal to a
    // memory id must still be two nodes.
    g.upsert_memory("shared", MemoryKind::Fact).unwrap();
    g.upsert_agent("shared").unwrap();
    g.upsert_session("shared").unwrap();
    assert_eq!(g.stats().unwrap()["nodes"], json!(3));

    let detail = g.neighbors_detail("shared").unwrap();
    assert!(detail.is_empty());
    let rows = g
        .cypher(
            "MATCH (n {id: $id}) RETURN labels(n) AS labels, n.mid AS mid",
            &json!({"id": "shared"}),
        )
        .unwrap();
    assert_eq!(rows, json!([{"labels": ["Memory"], "mid": "shared"}]));
}

#[test]
fn memory_without_hubs_gets_no_hub_edges() {
    let g = graph();
    let item = remem_types::MemoryItem::new(MemoryKind::Note, "bare".into());
    g.attach(&item).unwrap();
    assert_eq!(g.stats().unwrap()["nodes"], json!(1));
    assert_eq!(g.stats().unwrap()["edges"], json!(0));
}

#[test]
fn cypher_match_roundtrip_with_params() {
    let g = graph();
    g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    g.upsert_memory("m2", MemoryKind::Preference).unwrap();
    g.link("m1", "m2", "RELATES_TO").unwrap();

    let rows = g
        .cypher(
            "MATCH (a:Memory)-[r]->(b:Memory) \
             WHERE a.mid = $from RETURN b.mid AS mid, b.kind AS kind, type(r) AS rel",
            &json!({"from": "m1"}),
        )
        .unwrap();
    assert_eq!(
        rows,
        json!([{"mid": "m2", "kind": "preference", "rel": "RELATES_TO"}])
    );
}

#[test]
fn cypher_accepts_null_params_and_returns_nodes() {
    let g = graph();
    g.upsert_memory("m1", MemoryKind::Fact).unwrap();
    let rows = g.cypher("MATCH (n:Memory) RETURN n", &json!(null)).unwrap();
    let node = rows[0]["n"].clone();
    assert!(node["labels"]
        .as_array()
        .unwrap()
        .contains(&json!(MEMORY_LABEL)));
    assert_eq!(node["properties"]["mid"], json!("m1"));
    assert_eq!(node["properties"]["kind"], json!("fact"));
}

#[test]
fn cypher_rejects_non_object_params() {
    let g = graph();
    let err = g.cypher("MATCH (n) RETURN n", &json!(["m1"])).unwrap_err();
    assert!(err.to_string().contains("invalid params"), "{err}");
}

#[test]
fn survives_reopen_of_same_file() {
    let path = std::env::temp_dir().join(format!("remem-graph-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let g = Graph::open(&path).unwrap();
        g.upsert_memory("m1", MemoryKind::Fact).unwrap();
        g.upsert_memory("m2", MemoryKind::Fact).unwrap();
        g.link("m1", "m2", "RELATES_TO").unwrap();
    }
    let g = Graph::open(&path).unwrap();
    assert_eq!(
        sorted(g.neighbors("m1").unwrap()),
        vec![("m2".to_string(), "RELATES_TO".to_string())]
    );
    let _ = std::fs::remove_file(&path);
}

/// Hand-built graph exercising both problems `validate` looks for: an edge whose
/// target row is gone, and Memories with no memory-to-memory edge.
///
/// graphqlite opens its connection with `PRAGMA foreign_keys = ON` and declares
/// `ON DELETE CASCADE` on `edges`, so no [`Graph`] API can produce the dangling
/// row. It is written here with raw SQL after switching enforcement off, which is
/// what a foreign writer on the same file or schema drift would leave behind.
#[test]
fn validate_reports_dangling_edges_and_orphan_memories() {
    let g = Graph::open_in_memory().unwrap();
    let conn = g.sqlite();

    g.upsert_agent("a1").unwrap();
    for mid in ["m1", "m2"] {
        g.upsert_memory(mid, MemoryKind::Fact).unwrap();
        g.link(mid, "agent:a1", BELONGS_TO_AGENT).unwrap();
    }
    g.link("m1", "m2", "RELATES_TO").unwrap();
    g.link("m2", "m1", "SUPERSEDES").unwrap();
    assert_eq!(g.validate(), Vec::<String>::new(), "healthy graph");

    // Orphans: a Memory with no edges at all, and one with hub edges only.
    g.upsert_memory("m3", MemoryKind::Fact).unwrap();
    g.upsert_memory("m4", MemoryKind::Fact).unwrap();
    g.upsert_session("s1").unwrap();
    g.link("m4", "session:s1", BELONGS_TO_SESSION).unwrap();
    assert_eq!(
        g.validate(),
        vec![
            "orphan memory: m3 (no edges outside agent/session hubs)".to_string(),
            "orphan memory: m4 (no edges outside agent/session hubs)".to_string(),
        ]
    );

    conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
    let m1: i64 = node_id(conn, "mid", "m1");
    conn.execute(
        "INSERT INTO edges (source_id, target_id, type) VALUES (?1, 9999, 'RELATES_TO')",
        [m1],
    )
    .unwrap();
    assert_eq!(
        g.validate(),
        vec![
            "dangling edge: m1 -[:RELATES_TO]-> #9999 (missing target node)".to_string(),
            "orphan memory: m3 (no edges outside agent/session hubs)".to_string(),
            "orphan memory: m4 (no edges outside agent/session hubs)".to_string(),
        ]
    );
}

/// An edge with both ends gone is labelled by rowid; a surviving end keeps its
/// namespaced hub id rather than a bare row number.
#[test]
fn validate_names_the_ends_it_can() {
    let g = Graph::open_in_memory().unwrap();
    let conn = g.sqlite();
    g.upsert_agent("a1").unwrap();
    let agent: i64 = node_id(conn, "aid", "a1");
    conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
    conn.execute(
        "INSERT INTO edges (source_id, target_id, type) \
         VALUES (4242, 4243, 'RELATES_TO'), (4242, ?1, 'BELONGS_TO_AGENT')",
        [agent],
    )
    .unwrap();
    assert_eq!(
        g.validate(),
        vec![
            "dangling edge: #4242 -[:RELATES_TO]-> #4243 (missing source and target nodes)"
                .to_string(),
            "dangling edge: #4242 -[:BELONGS_TO_AGENT]-> agent:a1 (missing source node)"
                .to_string(),
        ]
    );
}

/// graphqlite rowid of the node carrying `key = value`.
fn node_id(conn: &rusqlite::Connection, key: &str, value: &str) -> i64 {
    conn.query_row(
        "SELECT p.node_id FROM node_props_text p JOIN property_keys k ON k.id = p.key_id \
         WHERE k.key = ?1 AND p.value = ?2",
        [key, value],
        |r| r.get(0),
    )
    .unwrap()
}
