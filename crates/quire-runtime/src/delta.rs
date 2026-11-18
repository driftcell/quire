//! Coalesce high-frequency thinking/text token deltas before persistence/SSE.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeltaKind {
    Thinking,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoalescedDelta {
    pub run_id: String,
    pub agent_id: String,
    pub kind: DeltaKind,
    pub delta: String,
}

#[derive(Debug)]
struct Buffer {
    kind: DeltaKind,
    text: String,
    started_at: Instant,
}

/// Buffers deltas per `(run_id, agent_id)`.
///
/// Buffers flush after 180ms or 100 characters. Callers must force a flush before any structural
/// event (`ToolCallStarted`, `TaskFocus`, `TurnFinished`, `TurnFailed`) or cancellation so event
/// order remains durable and replay-safe.
#[derive(Debug)]
pub struct DeltaCoalescer {
    buffers: BTreeMap<(String, String), Buffer>,
    max_delay: Duration,
    max_chars: usize,
}

impl Default for DeltaCoalescer {
    fn default() -> Self {
        Self::new(Duration::from_millis(180), 100)
    }
}

impl DeltaCoalescer {
    pub fn new(max_delay: Duration, max_chars: usize) -> Self {
        Self {
            buffers: BTreeMap::new(),
            max_delay,
            max_chars: max_chars.max(1),
        }
    }

    pub fn push(
        &mut self,
        now: Instant,
        run_id: &str,
        agent_id: &str,
        kind: DeltaKind,
        delta: &str,
    ) -> Vec<CoalescedDelta> {
        if delta.is_empty() {
            return Vec::new();
        }
        let key = (run_id.to_string(), agent_id.to_string());
        let mut flushed = Vec::new();
        if self
            .buffers
            .get(&key)
            .is_some_and(|buffer| buffer.kind != kind)
            && let Some(delta) = self.flush_key(&key)
        {
            flushed.push(delta);
        }
        let buffer = self.buffers.entry(key.clone()).or_insert_with(|| Buffer {
            kind,
            text: String::new(),
            started_at: now,
        });
        buffer.text.push_str(delta);
        let should_flush = buffer.text.chars().count() >= self.max_chars
            || now.duration_since(buffer.started_at) >= self.max_delay;
        if should_flush && let Some(delta) = self.flush_key(&key) {
            flushed.push(delta);
        }
        flushed
    }

    pub fn flush_due(&mut self, now: Instant) -> Vec<CoalescedDelta> {
        let keys: Vec<(String, String)> = self
            .buffers
            .iter()
            .filter(|(_, buffer)| now.duration_since(buffer.started_at) >= self.max_delay)
            .map(|(key, _)| key.clone())
            .collect();
        keys.iter().filter_map(|key| self.flush_key(key)).collect()
    }

    pub fn flush_key(&mut self, key: &(String, String)) -> Option<CoalescedDelta> {
        let buffer = self.buffers.remove(key)?;
        (!buffer.text.is_empty()).then(|| CoalescedDelta {
            run_id: key.0.clone(),
            agent_id: key.1.clone(),
            kind: buffer.kind,
            delta: buffer.text,
        })
    }

    /// Force-flush every agent/run in deterministic key order.
    pub fn flush_all(&mut self) -> Vec<CoalescedDelta> {
        let keys: Vec<(String, String)> = self.buffers.keys().cloned().collect();
        keys.iter().filter_map(|key| self.flush_key(key)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_until_character_threshold() {
        let mut coalescer = DeltaCoalescer::new(Duration::from_millis(180), 12);
        let start = Instant::now();
        assert!(
            coalescer
                .push(start, "run", "main", DeltaKind::Text, "Hello ")
                .is_empty()
        );
        assert!(
            coalescer
                .push(start, "run", "main", DeltaKind::Text, "world")
                .is_empty()
        );
        let flushed = coalescer.push(
            start + Duration::from_millis(20),
            "run",
            "main",
            DeltaKind::Text,
            "!!!",
        );
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].delta, "Hello world!!!");
    }

    #[test]
    fn kind_switch_forces_previous_flush() {
        let mut coalescer = DeltaCoalescer::default();
        let start = Instant::now();
        coalescer.push(start, "run", "main", DeltaKind::Thinking, "plan");
        let flushed = coalescer.push(
            start + Duration::from_millis(10),
            "run",
            "main",
            DeltaKind::Text,
            "answer",
        );
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].kind, DeltaKind::Thinking);
        assert_eq!(flushed[0].delta, "plan");
    }

    #[test]
    fn time_threshold_and_forced_flush_are_ordered() {
        let mut coalescer = DeltaCoalescer::default();
        let start = Instant::now();
        coalescer.push(start, "run_b", "agent_b", DeltaKind::Text, "b");
        coalescer.push(start, "run_a", "agent_a", DeltaKind::Thinking, "a");
        let due = coalescer.flush_due(start + Duration::from_millis(181));
        assert_eq!(due.len(), 2);
        assert_eq!(due[0].run_id, "run_a");
        assert_eq!(due[1].run_id, "run_b");

        coalescer.push(start, "run_c", "agent", DeltaKind::Text, "c");
        let forced = coalescer.flush_all();
        assert_eq!(forced.len(), 1);
        assert_eq!(forced[0].delta, "c");
        assert!(coalescer.flush_all().is_empty());
    }
}
