pub mod agents;
pub mod context_window;
pub mod cost;
pub mod directory;
pub mod environment;
pub mod git;
pub mod hooks;
pub mod model;
pub mod output_style;
pub mod session;
pub mod session_name;
pub mod todos;
pub mod skills;
pub mod tools;
pub mod update;
pub mod usage;

use crate::config::{InputData, SegmentId};
use std::collections::HashMap;

// New Segment trait for data collection only
pub trait Segment {
    fn collect(&self, input: &InputData) -> Option<SegmentData>;
    fn id(&self) -> SegmentId;
}

#[derive(Debug, Clone)]
pub struct SegmentData {
    pub primary: String,
    pub secondary: String,
    pub metadata: HashMap<String, String>,
}

// Re-export all segment types
pub use agents::AgentsSegment;
pub use context_window::ContextWindowSegment;
pub use cost::CostSegment;
pub use directory::DirectorySegment;
pub use environment::EnvironmentSegment;
pub use git::GitSegment;
pub use hooks::HooksSegment;
pub use model::ModelSegment;
pub use output_style::OutputStyleSegment;
pub use session::SessionSegment;
pub use session_name::SessionNameSegment;
pub use todos::TodosSegment;
pub use skills::SkillsSegment;
pub use tools::ToolsSegment;
pub use update::UpdateSegment;
pub use usage::UsageSegment;
