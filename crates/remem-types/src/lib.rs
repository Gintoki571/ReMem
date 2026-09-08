use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    Fact,
    Decision,
    Mistake,
    Preference,
    Event,
    Note,
}

impl MemoryKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "fact" => Some(Self::Fact),
            "decision" => Some(Self::Decision),
            "mistake" => Some(Self::Mistake),
            "preference" => Some(Self::Preference),
            "event" => Some(Self::Event),
            "note" => Some(Self::Note),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Decision => "decision",
            Self::Mistake => "mistake",
            Self::Preference => "preference",
            Self::Event => "event",
            Self::Note => "note",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: String,
    pub kind: MemoryKind,
    pub content: String,
    pub tags: Vec<String>,
    pub agent_id: String,
    pub session_id: String,
    pub importance: f32,
    pub created_at: i64,
    pub updated_at: i64,
    /// When the thing happened, as opposed to when we typed it. None for
    /// timeless memories. Absent in older payloads deserializes to None.
    #[serde(default)]
    pub occurred_at: Option<i64>,
}

impl MemoryItem {
    pub fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before epoch")
            .as_secs() as i64
    }

    pub fn new(kind: MemoryKind, content: String) -> Self {
        let now = Self::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            content,
            tags: Vec::new(),
            agent_id: String::new(),
            session_id: String::new(),
            importance: 0.5,
            created_at: now,
            updated_at: now,
            occurred_at: None,
        }
    }

    /// Clamp an importance value to 0.0..=1.0. NaN (and NULL on read) -> 0.5.
    pub fn clamp_importance(v: f32) -> f32 {
        if v.is_nan() {
            0.5
        } else {
            v.clamp(0.0, 1.0)
        }
    }

    /// Builder: set importance, clamped to 0.0..=1.0 (NaN -> 0.5).
    pub fn with_importance(mut self, v: f32) -> Self {
        self.importance = Self::clamp_importance(v);
        self
    }

    /// Setter: assign importance, clamped to 0.0..=1.0 (NaN -> 0.5).
    pub fn set_importance(&mut self, v: f32) {
        self.importance = Self::clamp_importance(v);
    }

    /// The event clock: when it happened if known, else when it was stored.
    pub fn event_time(&self) -> i64 {
        self.occurred_at.unwrap_or(self.created_at)
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecallQuery {
    pub text: String,
    pub k: usize,
    pub kinds: Option<Vec<MemoryKind>>,
    pub tags: Option<Vec<String>>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    /// Inclusive lower bound on the event clock (see MemoryItem::event_time).
    pub since: Option<i64>,
    /// Inclusive upper bound on the event clock.
    pub until: Option<i64>,
    /// Result budget in characters (content length). Hits that do not fit are
    /// skipped rather than truncating the tail. None means no budget.
    pub max_chars: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RecallHit {
    pub item: MemoryItem,
    pub score: f64,
    pub reasons: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importance_clamped_to_unit_range() {
        let mk = |v: f32| {
            MemoryItem::new(MemoryKind::Fact, "x".into()).with_importance(v).importance
        };
        assert_eq!(mk(999.0), 1.0);
        assert_eq!(mk(-5.0), 0.0);
        assert_eq!(mk(f32::NAN), 0.5);
        assert_eq!(mk(0.7), 0.7);
        assert_eq!(MemoryItem::clamp_importance(f32::INFINITY), 1.0);
    }
}
