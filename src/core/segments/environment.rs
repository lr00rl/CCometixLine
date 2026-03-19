use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct EnvironmentSegment;

impl EnvironmentSegment {
    pub fn new() -> Self {
        Self
    }

    fn count_claude_md(cwd: &str) -> u32 {
        let mut count = 0u32;

        // Check cwd/.claude/CLAUDE.md
        let cwd_claude = PathBuf::from(cwd).join(".claude");
        if cwd_claude.join("CLAUDE.md").exists() {
            count += 1;
        }

        // Check ~/.claude/CLAUDE.md
        if let Some(home) = dirs::home_dir() {
            if home.join(".claude").join("CLAUDE.md").exists() {
                count += 1;
            }
        }

        count
    }

    fn count_rules(cwd: &str) -> u32 {
        let mut count = 0u32;

        // Count ~/.claude/rules/*.md
        if let Some(home) = dirs::home_dir() {
            let rules_dir = home.join(".claude").join("rules");
            count += Self::count_md_files(&rules_dir);
        }

        // Count cwd/.claude/rules/*.md
        let cwd_rules = PathBuf::from(cwd).join(".claude").join("rules");
        count += Self::count_md_files(&cwd_rules);

        count
    }

    fn count_md_files(dir: &Path) -> u32 {
        if !dir.exists() {
            return 0;
        }
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|ext| ext.eq_ignore_ascii_case("md"))
                            .unwrap_or(false)
                    })
                    .count() as u32
            })
            .unwrap_or(0)
    }

    fn count_mcps_and_hooks(cwd: &str) -> (u32, u32) {
        let mut mcp_count = 0u32;
        let mut hook_count = 0u32;

        // Parse settings from ~/.claude/settings.json and cwd/.claude/settings.json
        let paths = vec![
            dirs::home_dir()
                .map(|h| h.join(".claude").join("settings.json")),
            Some(PathBuf::from(cwd).join(".claude").join("settings.json")),
        ];

        for path_opt in paths {
            let path = match path_opt {
                Some(p) => p,
                None => continue,
            };

            if !path.exists() {
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let value: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // Count MCP servers
            if let Some(mcps) = value.get("mcpServers").and_then(|v| v.as_object()) {
                mcp_count += mcps.len() as u32;
            }

            // Count hooks (hooks is an object with event keys, each containing arrays)
            if let Some(hooks) = value.get("hooks").and_then(|v| v.as_object()) {
                for (_event, handlers) in hooks {
                    if let Some(arr) = handlers.as_array() {
                        hook_count += arr.len() as u32;
                    }
                }
            }
        }

        (mcp_count, hook_count)
    }
}

impl Segment for EnvironmentSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let cwd = &input.workspace.current_dir;

        let claude_md_count = Self::count_claude_md(cwd);
        let rules_count = Self::count_rules(cwd);
        let (mcp_count, hook_count) = Self::count_mcps_and_hooks(cwd);

        let total = claude_md_count + rules_count + mcp_count + hook_count;
        if total == 0 {
            return None;
        }

        let mut parts = Vec::new();
        if claude_md_count > 0 {
            parts.push(format!("{} CLAUDE.md", claude_md_count));
        }
        if rules_count > 0 {
            parts.push(format!("{} rules", rules_count));
        }
        if mcp_count > 0 {
            parts.push(format!("{} MCPs", mcp_count));
        }
        if hook_count > 0 {
            parts.push(format!("{} hooks", hook_count));
        }

        let primary = parts.join(" · ");

        let mut metadata = HashMap::new();
        metadata.insert("claude_md".to_string(), claude_md_count.to_string());
        metadata.insert("rules".to_string(), rules_count.to_string());
        metadata.insert("mcps".to_string(), mcp_count.to_string());
        metadata.insert("hooks".to_string(), hook_count.to_string());

        Some(SegmentData {
            primary,
            secondary: String::new(),
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::Environment
    }
}
