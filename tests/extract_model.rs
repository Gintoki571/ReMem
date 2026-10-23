use neocortex::extract_model::extract_text;
use neocortex::temporal::Clock;

fn clock() -> Clock {
    Clock::system()
}

#[test]
fn extracts_subject_relation_and_negation_from_plain_text() {
    let result = extract_text(
        "Dr. Patel lives in Pune, but no longer works at Acme.",
        &clock(),
    );
    assert!(result.entities.iter().any(|e| e.name == "Dr. Patel"));
    assert!(result.entities.iter().any(|e| e.name == "Pune"));
    assert!(result
        .relations
        .iter()
        .any(|r| r.source == "Dr. Patel" && r.target == "Pune" && r.predicate == "lives in"));
    assert!(result
        .relations
        .iter()
        .any(|r| r.target == "Acme" && r.negated == Some(true)));
}

#[test]
fn extracts_sentence_initial_single_name_and_explicit_date() {
    let result = extract_text("Alicia lives in Pune since 2025-03-05.", &clock());
    assert!(result.entities.iter().any(|e| e.name == "Alicia"));
    assert_eq!(result.valid_from.as_deref(), Some("2025-03-05"));
}

#[test]
fn retraction_is_one_relation_and_since_ends_the_target() {
    let result = extract_text(
        "Ivan no longer works at Acme; Alicia lives in Jaipur since 2025-03-05.",
        &clock(),
    );
    assert!(!result.entities.iter().any(|e| e.name == "longer"));
    assert_eq!(
        result
            .relations
            .iter()
            .filter(|r| r.predicate == "works at")
            .count(),
        1
    );
    assert!(result
        .relations
        .iter()
        .any(|r| r.target == "Jaipur" && r.predicate == "lives in"));
}
