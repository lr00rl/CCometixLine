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
                            tool_map.insert(
                                id.clone(),
                                ToolRecord {
                                    id: id.clone(),
                                    name: name.clone(),
                                    completed: false,
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
        let tools = Self::parse_tools(&input.transcript_path);

        if tools.is_empty() {
            return None;
        }

        let running: Vec<&ToolRecord> = tools.iter().filter(|t| !t.completed).collect();
        let completed: Vec<&ToolRecord> = tools.iter().filter(|t| t.completed).collect();

        let mut parts: Vec<String> = Vec::new();

        // Show up to 2 running tools
        for tool in running.iter().take(2) {
            let short = Self::shorten_tool_name(&tool.name);
            parts.push(format!("◐ {}", short));
        }

        // Show completed tools grouped by name (up to 4)
        if !completed.is_empty() {
            let mut counts: HashMap<String, u32> = HashMap::new();
            for tool in &completed {
                *counts.entry(tool.name.clone()).or_insert(0) += 1;
            }
            let mut count_vec: Vec<(String, u32)> = counts.into_iter().collect();
            count_vec.sort_by(|a, b| b.1.cmp(&a.1)); // sort by frequency
            for (name, count) in count_vec.iter().take(4) {
                let short = Self::shorten_tool_name(name);
                if *count > 1 {
                    parts.push(format!("✓ {} ×{}", short, count));
                } else {
                    parts.push(format!("✓ {}", short));
                }
            }
        }

        if parts.is_empty() {
            return None;
        }

        let primary = parts.join(" ");

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
