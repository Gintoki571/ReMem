use neocortex::embed::HashEmbedder;
use neocortex::engine::{Engine, RecallInput, RememberInput};
use neocortex::temporal::Clock;
use std::sync::Arc;

#[test]
fn keyword_recall_returns_matching_memory() {
    let engine = Engine::open_in_memory(Arc::new(HashEmbedder))
        .unwrap()
        .with_clock(Clock {
            now: 1_800_000_000,
            tz_offset_secs: 0,
        });
    engine
        .remember(RememberInput {
            content: "The production database runs PostgreSQL".into(),
            ..Default::default()
        })
        .unwrap();
    let out = engine
        .recall(&RecallInput {
            query: "production database".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(out.memories.len(), 1);
    assert!(out.memories[0].content.contains("PostgreSQL"));
}
