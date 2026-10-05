//! Data-agent-owned infrastructure. Implement Core ports here; keep SQLite row types and CLI schemas private.
mod claude;
mod process;
mod sqlite;

pub use claude::ClaudeCodeAdapter;
pub use process::{ProcessRunner, RunnerLimits};
pub use sqlite::SqliteRepository;
