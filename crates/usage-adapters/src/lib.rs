//! Data-agent-owned infrastructure. Implement Core ports here; keep SQLite row types and CLI schemas private.
mod agents;
mod claude;
mod process;
mod sqlite;
mod usage_snapshot;

pub use agents::{validate_agent_directory, AgentAdapter};
pub use claude::ClaudeCodeAdapter;
pub use process::{AgentKind, ProcessRunner, RunnerLimits};
pub use sqlite::SqliteRepository;
