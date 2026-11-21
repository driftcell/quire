//! Scoped agent-event projection and delivery.
//!
//! The rig provider produces runtime events. This module is the only place that
//! attributes them to a run scope, translates them to the session wire contract,
//! coalesces deltas and persists the resulting events.

mod log;
mod project;
mod scope;
mod sink;

pub use log::SessionEventLog;
pub use project::{EventAction, project};
pub use scope::EventScope;
pub use sink::{ApplyOutcome, EventSink};
