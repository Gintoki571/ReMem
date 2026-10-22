use neocortex::{
    embed::HashEmbedder,
    engine::{Engine, RememberInput},
};
use std::sync::Arc;

#[test]
fn remember_stores_memory_and_embedder_identity() {
    let engine = Engine::open_in_memory(Arc::new(HashEmbedder))
        .unwrap()
        .with_clock(neocortex::temporal::Clock {
            now: 1_700_000_000,
            tz_offset_secs: 0,
        });

    let output = engine
        .remember(RememberInput {
            content: "The deployment uses Postgres".to_string(),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(output.id, 1);
    assert_eq!(
        engine
            .db
            .sql()
            .query_row("SELECT value FROM meta WHERE key = 'embedder'", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap(),
        "hash"
    );
}

#[test]
fn duplicate_returns_existing_id_and_kind_validation_is_explicit() {
    let engine = Engine::open_in_memory(Arc::new(HashEmbedder)).unwrap();
    let input = RememberInput {
        content: "same durable fact".into(),
        ..Default::default()
    };
    let first = engine.remember(input.clone()).unwrap();
    let second = engine.remember(input).unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(second.duplicate_of, Some(first.id));
    let error = engine
        .remember(RememberInput {
            content: "bad".into(),
            kind: Some("unsupported".into()),
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
    assert!(error.contains("fact") && error.contains("note"));
}

#[test]
fn embedder_identity_rejects_a_different_name() {
    struct Other;
    impl neocortex::embed::Embedder for Other {
        fn embed(
            &self,
            texts: &[&str],
            mode: neocortex::embed::Mode,
        ) -> neocortex::Result<Vec<Vec<f32>>> {
            HashEmbedder.embed(texts, mode)
        }
        fn name(&self) -> &str {
            "other"
        }
    }
    let engine = Engine::open_in_memory(Arc::new(HashEmbedder)).unwrap();
    engine
        .remember(RememberInput {
            content: "identity".into(),
            ..Default::default()
        })
        .unwrap();
    let other = Engine {
        db: engine.db,
        emb: Arc::new(Other),
        cfg: Default::default(),
        fixed_clock: None,
    };
    assert!(other
        .remember(RememberInput {
            content: "second".into(),
            ..Default::default()
        })
        .unwrap_err()
        .to_string()
        .contains("built with embedder 'hash'"));
}
