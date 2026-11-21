use std::fmt;

/// Complete attribution for one model event and its session wire envelope.
///
/// For subagent activity, `run_id` is the subagent run and `parent_run_id` is
/// the owning root run used for trace grouping. Keeping all three values in the
/// coalescer key prevents subagent deltas from being promoted to top-level runs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventScope {
    pub run_id: String,
    pub agent_id: String,
    pub parent_run_id: Option<String>,
}

impl EventScope {
    pub fn main(run_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            agent_id: "main".into(),
            parent_run_id: None,
        }
    }

    pub fn subagent(parent: &Self, run_id: impl Into<String>, agent_id: impl Into<String>) -> Self {
        Self {
            run_id: run_id.into(),
            agent_id: agent_id.into(),
            parent_run_id: Some(parent.root_run_id().to_string()),
        }
    }

    /// Main-agent events have no parent envelope; subagent events always do.
    pub fn is_main(&self) -> bool {
        self.parent_run_id.is_none()
    }

    /// Root run whose trace owns this event.
    pub fn root_run_id(&self) -> &str {
        self.parent_run_id.as_deref().unwrap_or(&self.run_id)
    }
}

impl fmt::Display for EventScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.root_run_id(), self.run_id)
    }
}
