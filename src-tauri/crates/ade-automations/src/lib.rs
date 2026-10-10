//! Local automation core (spec §3.1): storage, schedule math, 60s tick,
//! precheck, and startup reconciliation live here. The renderer keeps its
//! existing dispatch and execution paths unchanged.

pub mod precheck;
pub mod schedule;
pub mod service;
pub mod store;

pub use schedule::{
    is_valid_schedule, latest_occurrence_at_or_before, next_occurrence_after, ScheduleError,
    AUTOMATION_CRON_EXPRESSION_MAX_BYTES,
};
pub use service::{AutomationService, EventSink};
pub use store::{AutomationStore, RunsPage, StoreError, MAX_RUNS_PER_AUTOMATION};
