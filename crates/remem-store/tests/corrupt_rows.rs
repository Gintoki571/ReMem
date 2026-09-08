use remem_store::Store;
use remem_types::{MemoryItem, MemoryKind};

fn item(kind: MemoryKind, content: &str) -> MemoryItem {
    let mut m = MemoryItem::new(kind, content.to_string());
    m.tags = vec!["t".into()];
    m
}

fn rowid_of(store: &Store, id: &str) -> i64 {
    store
        .connection()
        .query_row(
            "SELECT rowid FROM memories WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap()
}

fn corrupt(store: &Store, id: &str, sql_set: &str) {
    store
        .connection()
        .execute(
            &format!("UPDATE memories SET {sql_set} WHERE id = ?1"),
            rusqlite::params![id],
        )
        .unwrap();
}

#[test]
fn all_six_kinds_roundtrip() {
    let store = Store::open(":memory:").unwrap();
    let kinds = [
        MemoryKind::Fact,
        MemoryKind::Decision,
        MemoryKind::Mistake,
        MemoryKind::Preference,
        MemoryKind::Event,
        MemoryKind::Note,
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        let id = store.insert(&item(kind, &format!("kind row {i}"))).unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap().kind, kind);
    }
    assert_eq!(store.list(false).unwrap().len(), 6);
}

#[test]
fn empty_tags_stay_valid() {
    let store = Store::open(":memory:").unwrap();
    let mut m = item(MemoryKind::Note, "no tags here");
    m.tags = vec![];
    let id = store.insert(&m).unwrap();
    assert_eq!(store.get(&id).unwrap().unwrap().tags, Vec::<String>::new());
    assert_eq!(store.list(false).unwrap().len(), 1);
    assert_eq!(store.fts_search("no tags here", 5).unwrap().len(), 1);
}

#[test]
fn corrupt_kind_surfaces_as_err_naming_row() {
    let store = Store::open(":memory:").unwrap();
    let id = store
        .insert(&item(MemoryKind::Note, "bogus kind row"))
        .unwrap();
    corrupt(&store, &id, "kind = 'bogus'");
    let rowid = rowid_of(&store, &id);
    let err = format!("{:?}", store.get(&id).unwrap_err());
    assert!(err.contains(&rowid.to_string()), "get err names row: {err}");
    let err = format!("{:?}", store.list(false).unwrap_err());
    assert!(
        err.contains(&rowid.to_string()),
        "list err names row: {err}"
    );
}

#[test]
fn corrupt_tags_surface_as_err_naming_row() {
    let store = Store::open(":memory:").unwrap();
    let id = store
        .insert(&item(MemoryKind::Fact, "bogus tags row"))
        .unwrap();
    corrupt(&store, &id, "tags = '{{{'");
    let rowid = rowid_of(&store, &id);
    let err = format!("{:?}", store.get(&id).unwrap_err());
    assert!(err.contains(&rowid.to_string()), "get err names row: {err}");
    let err = format!("{:?}", store.list(false).unwrap_err());
    assert!(
        err.contains(&rowid.to_string()),
        "list err names row: {err}"
    );
}

#[test]
fn corrupt_row_fails_fts_search() {
    let store = Store::open(":memory:").unwrap();
    let id = store
        .insert(&item(MemoryKind::Note, "uniqueword corrupt fts"))
        .unwrap();
    corrupt(&store, &id, "kind = 'bogus'");
    let rowid = rowid_of(&store, &id);
    let err = format!("{:?}", store.fts_search("uniqueword", 10).unwrap_err());
    assert!(err.contains(&rowid.to_string()), "fts err names row: {err}");
}
