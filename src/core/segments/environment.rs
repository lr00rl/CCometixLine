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

    fn get_claude_md_paths(cwd: &str) -> Vec<String> {
        let mut paths = Vec::new();
        if let Some(home) = dirs::home_dir() {
            let p = home.join(".claude").join("CLAUDE.md");
            if p.exists() {
                paths.push("~/.claude/CLAUDE.md".to_string());
            }
        }
        let p = PathBuf::from(cwd).join(".claude").join("CLAUDE.md");
        if p.exists() {
            paths.push(".claude/CLAUDE.md".to_string());
        }
        paths
    }

    fn get_rule_names(cwd: &str) -> Vec<String> {
        let mut names = Vec::new();
        let dirs_to_check = [
            dirs::home_dir().map(|h| h.join(".claude").join("rules")),
            Some(PathBuf::from(cwd).join(".claude").join("rules")),
        ];
        for dir_opt in dirs_to_check {
            if let Some(dir) = dir_opt {
                names.extend(Self::md_filestems(&dir));
            }
        }
        names
    }

    fn md_filestems(dir: &Path) -> Vec<String> {
        if !dir.exists() {
            return Vec::new();
        }
        let mut names: Vec<String> = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let p = e.path();
                        if p.extension().and_then(|s| s.to_str()).map(|x| x.eq_ignore_ascii_case("md")).unwrap_or(false) {
                            p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn get_mcp_and_hook_names(cwd: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut mcp_names: Vec<String> = Vec::new();
        let mut hook_events: Vec<String> = Vec::new(); // event names
        let mut hook_cmds: Vec<String> = Vec::new();   // hook command snippets

        let paths = [
            dirs::home_dir().map(|h| h.join(".claude").join("settings.json")),
            Some(PathBuf::from(cwd).join(".claude").join("settings.json")),
        ];

        for path_opt in &paths {
            let path = match path_opt {
                Some(p) => p,
                None => continue,
            };
            if !path.exists() {
                continue;
            }
            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let value: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(mcps) = value.get("mcpServers").and_then(|v| v.as_object()) {
                for name in mcps.keys() {
                    if !mcp_names.contains(name) {
                        mcp_names.push(name.clone());
                    }
                }
            }

            if let Some(hooks) = value.get("hooks").and_then(|v| v.as_object()) {
                for (event, handlers) in hooks {
                    if let Some(arr) = handlers.as_array() {
                        for handler in arr {
                            // event name (deduplicated)
                            if !hook_events.contains(event) {
                                hook_events.push(event.clone());
                            }
                            // Try to get the command text for detail display
                            let cmd = handler
                                .get("hooks")
                                .and_then(|h| h.as_array())
                                .and_then(|a| a.first())
                                .and_then(|h| h.get("command"))
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .trim();
                            if !cmd.is_empty() {
                                // Shorten: take first word/token as identifier
                                let short = cmd.split_whitespace().next().unwrap_or(cmd);
                                let short = short.rsplit('/').next().unwrap_or(short); // basename
                                let label = format!("{}: {}", Self::shorten_event(event), short);
                                if !hook_cmds.contains(&label) {
                                    hook_cmds.push(label);
                                }
                            }
                        }
                    }
                }
            }
        }

        mcp_names.sort();
        hook_events.sort();
        (mcp_names, hook_events, hook_cmds)
    }

    fn shorten_event(event: &str) -> &str {
        match event {
            "PreToolUse" => "pre",
            "PostToolUse" => "post",
            "Notification" => "notify",
            "Stop" => "stop",
            "SubagentStop" => "sub-stop",
            other => other,
        }
    }
}

impl Segment for EnvironmentSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let cwd = &input.workspace.current_dir;
        crate::log_debug!("environment: scanning cwd={:?}", cwd);

        let config = crate::config::Config::load().ok();
        let show_names = config.as_ref()
            .and_then(|c| c.segments.iter().find(|s| s.id == SegmentId::Environment))
            .and_then(|sc| sc.options.get("show_names"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let claude_md_paths = Self::get_claude_md_paths(cwd);
        let rule_names = Self::get_rule_names(cwd);
        let (mcp_names, hook_events, hook_cmds) = Self::get_mcp_and_hook_names(cwd);

        crate::log_debug!(
            "environment: claude_md={:?} rules={:?} mcps={:?} hooks={:?}",
            claude_md_paths, rule_names, mcp_names, hook_events
        );

        let total = claude_md_paths.len() + rule_names.len() + mcp_names.len() + hook_events.len();
        if total == 0 {
            crate::log_debug!("environment: no CLAUDE.md, rules, MCPs, or hooks found, returning None");
            return None;
        }

        let primary = if show_names {
            // Detail mode: list names
            let mut parts = Vec::new();
            if !claude_md_paths.is_empty() {
                parts.push(format!("CLAUDE.md: {}", claude_md_paths.join(", ")));
            }
            if !rule_names.is_empty() {
                parts.push(format!("rules: {}", rule_names.join(", ")));
            }
            if !mcp_names.is_empty() {
                parts.push(format!("MCPs: {}", mcp_names.join(", ")));
            }
            if !hook_cmds.is_empty() {
                parts.push(format!("hooks: {}", hook_cmds.join(", ")));
            } else if !hook_events.is_empty() {
                parts.push(format!("hooks: {}", hook_events.join(", ")));
            }
            parts.join(" | ")
        } else {
            // Compact mode: counts
            let mut parts = Vec::new();
            if !claude_md_paths.is_empty() {
                parts.push(format!("{} CLAUDE.md", claude_md_paths.len()));
            }
            if !rule_names.is_empty() {
                parts.push(format!("{} rules", rule_names.len()));
            }
            if !mcp_names.is_empty() {
                parts.push(format!("{} MCPs", mcp_names.len()));
            }
            if !hook_events.is_empty() {
                parts.push(format!("{} hooks", hook_events.len()));
            }
            parts.join(" · ")
        };

        let mut metadata = HashMap::new();
        metadata.insert("claude_md".to_string(), claude_md_paths.len().to_string());
        metadata.insert("rules".to_string(), rule_names.len().to_string());
        metadata.insert("mcps".to_string(), mcp_names.len().to_string());
        metadata.insert("hooks".to_string(), hook_events.len().to_string());

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
