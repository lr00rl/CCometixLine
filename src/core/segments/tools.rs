use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId, TranscriptEntry};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug)]
struct ToolRecord {
    id: String,
    name: String,
    completed: bool,
    /// Key argument: file path, bash command, search pattern, etc.
    arg: Option<String>,
}

#[derive(Default)]
pub struct ToolsSegment;

impl ToolsSegment {
    pub fn new() -> Self {
        Self
    }

    fn parse_tools(transcript_path: &str) -> Vec<ToolRecord> {
        let path = Path::new(transcript_path);
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut tool_map: HashMap<String, ToolRecord> = HashMap::new();

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let entry: TranscriptEntry = match serde_json::from_str(line) {
                Ok(e) => e,
                Err(_) => continue,
            };

            let message = match &entry.message {
                Some(m) => m,
                None => continue,
            };

            let blocks = match &message.content {
                Some(b) => b,
                None => continue,
            };

            for block in blocks {
                match block.r#type.as_str() {
                    "tool_use" => {
                        if let (Some(id), Some(name)) = (&block.id, &block.name) {
                            // Skip Skill tool — handled by SkillsSegment
                            if name == "Skill" {
                                continue;
                            }
                            let arg = block.input.as_ref().and_then(|v| {
                                Self::extract_key_arg(name, v)
                            });
                            tool_map.insert(
                                id.clone(),
                                ToolRecord {
                                    id: id.clone(),
                                    name: name.clone(),
                                    completed: false,
                                    arg,
                                },
                            );
                        }
                    }
                    "tool_result" => {
                        if let Some(tool_use_id) = &block.tool_use_id {
                            if let Some(record) = tool_map.get_mut(tool_use_id) {
                                record.completed = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Return last 100 tool records
        let mut records: Vec<ToolRecord> = tool_map.into_values().collect();
        // Sort by id (lexicographic approximates insertion order for UUID-like ids)
        records.sort_by(|a, b| a.id.cmp(&b.id));
        if records.len() > 100 {
            records = records.into_iter().rev().take(100).collect();
            records.reverse();
        }
        records
    }

    /// Extract the most informative single argument from tool input JSON.
    fn extract_key_arg(tool_name: &str, input: &serde_json::Value) -> Option<String> {
        let obj = input.as_object()?;
        let raw = match tool_name {
            // File operations: use file_path
            "Read" | "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
                obj.get("file_path").or_else(|| obj.get("notebook_path"))
                    .and_then(|v| v.as_str())
                    .map(|s| Self::shorten_path(s))
            }
            // Shell: use command (truncated)
            "Bash" => {
                obj.get("command")
                    .and_then(|v| v.as_str())
                    .map(|s| {
                        let s = s.trim();
                        if s.len() > 35 {
                            format!("{}…", &s[..34])
                        } else {
                            s.to_string()
                        }
                    })
            }
            // Search: use pattern
            "Glob" | "Grep" => {
                obj.get("pattern")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
            // Subagent: use subagent_type
            "Task" | "Agent" => {
                obj.get("subagent_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
            // Web: use domain from url
            "WebFetch" => {
                obj.get("url")
                    .and_then(|v| v.as_str())
                    .and_then(|url| {
                        url.split("//").nth(1)
                            .map(|host| host.split('/').next().unwrap_or(host).to_string())
                    })
            }
            _ => None,
        };
        raw
    }

    /// Shorten a file path to just the filename (or last 2 components if needed).
    fn shorten_path(path: &str) -> String {
        std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string()
    }

    fn shorten_tool_name(name: &str) -> String {
        // Map common tool names to short names
        match name {
            "Read" | "read_file" => "Read".to_string(),
            "Write" | "write_file" => "Write".to_string(),
            "Edit" | "edit_file" => "Edit".to_string(),
            "Bash" | "bash" => "Bash".to_string(),
            "Glob" | "glob" => "Glob".to_string(),
            "Grep" | "grep" => "Grep".to_string(),
            "Task" | "task" => "Task".to_string(),
            "TodoWrite" | "todo_write" => "Todo".to_string(),
            "WebFetch" | "web_fetch" => "Web".to_string(),
            "WebSearch" | "web_search" => "Search".to_string(),
            other => {
                // Truncate long names
                if other.len() > 8 {
                    format!("{}…", &other[..7])
                } else {
                    other.to_string()
                }
            }
        }
    }
}

impl Segment for ToolsSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let config = crate::config::Config::load().ok();
        let show_args = config.as_ref()
            .and_then(|c| c.segments.iter().find(|s| s.id == SegmentId::Tools))
            .and_then(|sc| sc.options.get("show_args"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let tools = Self::parse_tools(&input.transcript_path);

        if tools.is_empty() {
            return None;
        }

        let running: Vec<&ToolRecord> = tools.iter().filter(|t| !t.completed).collect();
        let completed: Vec<&ToolRecord> = tools.iter().filter(|t| t.completed).collect();

        let primary = if show_args {
            // Detail mode: show recent tools chronologically with their key argument
            let mut parts: Vec<String> = Vec::new();
            // Running tools first
            for tool in running.iter().take(2) {
                let short = Self::shorten_tool_name(&tool.name);
                if let Some(ref arg) = tool.arg {
                    parts.push(format!("◐ {} {}", short, arg));
                } else {
                    parts.push(format!("◐ {}", short));
                }
            }
            // Last N completed tools (chronological, most recent last)
            let max_detail = 6usize.saturating_sub(parts.len());
            let recent_completed: Vec<_> = completed.iter()
                .rev()
                .take(max_detail)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            for tool in recent_completed {
                let short = Self::shorten_tool_name(&tool.name);
                if let Some(ref arg) = tool.arg {
                    parts.push(format!("✓ {} {}", short, arg));
                } else {
                    parts.push(format!("✓ {}", short));
                }
            }
            parts.join(" | ")
        } else {
            // Compact mode: grouped counts (original behavior)
            let mut parts: Vec<String> = Vec::new();
            for tool in running.iter().take(2) {
                let short = Self::shorten_tool_name(&tool.name);
                parts.push(format!("◐ {}", short));
            }
            if !completed.is_empty() {
                let mut counts: HashMap<String, u32> = HashMap::new();
                for tool in &completed {
                    *counts.entry(tool.name.clone()).or_insert(0) += 1;
                }
                let mut count_vec: Vec<(String, u32)> = counts.into_iter().collect();
                count_vec.sort_by(|a, b| b.1.cmp(&a.1));
                for (name, count) in count_vec.iter().take(4) {
                    let short = Self::shorten_tool_name(name);
                    if *count > 1 {
                        parts.push(format!("✓ {} ×{}", short, count));
                    } else {
                        parts.push(format!("✓ {}", short));
                    }
                }
            }
            parts.join(" ")
        };

        if primary.is_empty() {
            return None;
        }

        let mut metadata = HashMap::new();
        metadata.insert("running".to_string(), running.len().to_string());
        metadata.insert("completed".to_string(), completed.len().to_string());

        Some(SegmentData {
            primary,
            secondary: String::new(),
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::Tools
    }
}
