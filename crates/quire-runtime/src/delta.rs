//! Coalesce high-frequency thinking/text token deltas before persistence/SSE.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use crate::events::EventScope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeltaKind {
    Thinking,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoalescedDelta {
    /// Full wire attribution, including the root `parent_run_id` for subagents.
    pub scope: EventScope,
    pub kind: DeltaKind,
    pub delta: String,
}

#[derive(Debug)]
struct Buffer {
    kind: DeltaKind,
    text: String,
    started_at: Instant,
}

/// Buffers deltas per complete event scope.
///
/// Buffers flush after 50ms or 64 characters so the browser receives text at frame-friendly
/// intervals. Callers must force a flush before any structural event (`ToolCallStarted`,
/// `TaskFocus`, `TurnFinished`, `TurnFailed`) or cancellation so event order remains durable and
/// replay-safe. Scopes are independent: coalescing never merges main and subagent output.
#[derive(Debug)]
pub struct DeltaCoalescer {
    buffers: BTreeMap<EventScope, Buffer>,
    max_delay: Duration,
    max_chars: usize,
}

impl Default for DeltaCoalescer {
    fn default() -> Self {
        Self::new(Duration::from_millis(50), 64)
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
        scope: &EventScope,
        kind: DeltaKind,
        delta: &str,
    ) -> Vec<CoalescedDelta> {
        if delta.is_empty() {
            return Vec::new();
        }
        let key = scope.clone();
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
        let keys: Vec<EventScope> = self
            .buffers
            .iter()
            .filter(|(_, buffer)| now.duration_since(buffer.started_at) >= self.max_delay)
            .map(|(key, _)| key.clone())
            .collect();
        keys.iter().filter_map(|key| self.flush_key(key)).collect()
    }

    pub fn flush_key(&mut self, key: &EventScope) -> Option<CoalescedDelta> {
        let buffer = self.buffers.remove(key)?;
        (!buffer.text.is_empty()).then(|| CoalescedDelta {
            scope: key.clone(),
            kind: buffer.kind,
            delta: buffer.text,
        })
    }

    /// Force-flush every agent/run in deterministic scope order.
    pub fn flush_all(&mut self) -> Vec<CoalescedDelta> {
        let keys: Vec<EventScope> = self.buffers.keys().cloned().collect();
        keys.iter().filter_map(|key| self.flush_key(key)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_until_character_threshold() {
        let mut coalescer = DeltaCoalescer::new(Duration::from_millis(180), 12);
        let scope = EventScope::main("run");
        let start = Instant::now();
        assert!(
            coalescer
                .push(start, &scope, DeltaKind::Text, "Hello ")
                .is_empty()
        );
        assert!(
            coalescer
                .push(start, &scope, DeltaKind::Text, "world")
                .is_empty()
        );
        let flushed = coalescer.push(
            start + Duration::from_millis(20),
            &scope,
            DeltaKind::Text,
            "!!!",
        );
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].delta, "Hello world!!!");
    }

    #[test]
    fn kind_switch_forces_previous_flush() {
        let mut coalescer = DeltaCoalescer::default();
        let scope = EventScope::main("run");
        let start = Instant::now();
        coalescer.push(start, &scope, DeltaKind::Thinking, "plan");
        let flushed = coalescer.push(
            start + Duration::from_millis(10),
            &scope,
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
        let run_b = EventScope::main("run_b");
        let run_a = EventScope::main("run_a");
        let run_c = EventScope::main("run_c");
        coalescer.push(start, &run_b, DeltaKind::Text, "b");
        coalescer.push(start, &run_a, DeltaKind::Thinking, "a");
        let due = coalescer.flush_due(start + Duration::from_millis(181));
        assert_eq!(due.len(), 2);
        assert_eq!(due[0].scope.run_id, "run_a");
        assert_eq!(due[1].scope.run_id, "run_b");

        coalescer.push(start, &run_c, DeltaKind::Text, "c");
        let forced = coalescer.flush_all();
        assert_eq!(forced.len(), 1);
        assert_eq!(forced[0].delta, "c");
        assert!(coalescer.flush_all().is_empty());
    }

    #[test]
    fn subagent_flush_retains_the_root_parent_scope() {
        let mut coalescer = DeltaCoalescer::default();
        let main = EventScope::main("run_main");
        let child = EventScope::subagent(&main, "run_sub_1", "alpha");
        coalescer.push(Instant::now(), &child, DeltaKind::Text, "child output");

        let flushed = coalescer.flush_all();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].scope.run_id, "run_sub_1");
        assert_eq!(flushed[0].scope.agent_id, "alpha");
        assert_eq!(flushed[0].scope.parent_run_id.as_deref(), Some("run_main"));
    }
}
