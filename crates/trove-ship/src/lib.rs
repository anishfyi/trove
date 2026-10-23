//! trove-ship: turn a project and its trove into developer handoff docs with
//! private information stripped before anything is written.
//!
//! Pipeline: index (trove-index) -> collect facts -> compose docs -> redact
//! (deterministic detectors, `.troveignore`, optional agent review) -> render
//! (Markdown folder, single file or static site) -> fail-closed verification ->
//! write docs plus a redaction report.

pub mod cli;
pub mod collect;
pub mod docs;
pub mod output;
pub mod redact;
pub mod render;
pub mod report;
pub mod ship;

pub use output::Format;
pub use ship::{ship, FailClosed, Reindex, ShipOptions, ShipOutcome};
