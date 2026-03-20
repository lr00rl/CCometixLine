use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId, TranscriptEntry};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone)]
struct Todo {
    id: String,
    content: String,
    status: String, // "pending", "in_progress", "completed"
    #[allow(dead_code)]
    priority: String,
}

#[derive(Default)]
pub struct TodosSegment;

impl TodosSegment {
    pub fn new() -> Self {
        Self
    }

    fn parse_todos(transcript_path: &str) -> Vec<Todo> {
        let path = Path::new(transcript_path);
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut latest_todos: Vec<Todo> = Vec::new();
        // Counter for TaskCreate: generates sequential IDs "1","2","3"...
        // matching the taskId values used by TaskUpdate.
        let mut task_seq: u32 = 0;

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
                if block.r#type.as_str() != "tool_use" {
                    continue;
                }
                let tool_name = block.name.as_deref().unwrap_or("");

                // Handle TodoWrite tool — replaces entire list
                if tool_name == "TodoWrite" {
                    if let Some(input) = &block.input {
                        if let Some(todos_arr) = input.get("todos").and_then(|v| v.as_array()) {
                            let todos: Vec<Todo> = todos_arr
                                .iter()
                                .filter_map(|t| {
                                    let id = t.get("id")?.as_str()?.to_string();
                                    let content = t.get("content")?.as_str()?.to_string();
                                    let status = t
                                        .get("status")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("pending")
                                        .to_string();
                                    let priority = t
                                        .get("priority")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("medium")
                                        .to_string();
                                    Some(Todo { id, content, status, priority })
                                })
                                .collect();
                            if !todos.is_empty() {
                                latest_todos = todos;
                                // Reset counter so subsequent TaskCreate IDs
                                // continue from where TodoWrite left off.
                                task_seq = 0;
                            }
                        }
                    }
                }

                // Handle TaskCreate — assign sequential ID "1","2","3"...
                // so TaskUpdate's numeric taskId correctly lines up.
                if tool_name == "TaskCreate" {
                    if let Some(input) = &block.input {
                        let content = input
                            .get("subject")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !content.is_empty() {
                            task_seq += 1;
                            let id = task_seq.to_string();
                            latest_todos.push(Todo {
                                id,
                                content,
                                status: "pending".to_string(),
                                priority: "medium".to_string(),
                            });
                        }
                    }
                }

                // Handle TaskUpdate — match by numeric taskId
                if tool_name == "TaskUpdate" {
                    if let Some(input) = &block.input {
                        let task_id = input
                            .get("taskId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let new_status = input
                            .get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !task_id.is_empty() && !new_status.is_empty() {
                            if new_status == "deleted" {
                                // Remove deleted tasks entirely
                                latest_todos.retain(|t| t.id != task_id);
                            } else {
                                for todo in &mut latest_todos {
                                    if todo.id == task_id {
                                        todo.status = new_status.clone();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Log final state
        for todo in &latest_todos {
            crate::log_debug!(
                "todos: id={} status={} content={:?}",
                todo.id, todo.status, todo.content
            );
        }

        latest_todos
    }
}

impl TodosSegment {
    /// Truncate to max grapheme clusters, appending … if cut.
    fn trunc(s: &str, max: usize) -> String {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() <= max {
            s.to_string()
        } else {
            format!("{}…", chars[..max - 1].iter().collect::<String>())
        }
    }
}

impl Segment for TodosSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        crate::log_debug!("todos: reading transcript {:?}", input.transcript_path);
        let todos = Self::parse_todos(&input.transcript_path);

        if todos.is_empty() {
            crate::log_debug!("todos: no TaskCreate/TodoWrite entries found in transcript, returning None");
            return None;
        }

        let total = todos.len();
        let completed_count = todos.iter().filter(|t| t.status == "completed").count();
        let pending_count = todos.iter().filter(|t| t.status == "pending").count();
        let in_progress: Vec<&Todo> = todos.iter().filter(|t| t.status == "in_progress").collect();
        crate::log_debug!(
            "todos: total={} completed={} in_progress={} pending={}",
            total, completed_count, in_progress.len(), pending_count
        );

        let primary = if let Some(current) = in_progress.first() {
            // Find current's position to locate prev/next neighbours
            let cur_idx = todos.iter().position(|t| t.id == current.id).unwrap_or(0);

            // prev = last completed task before current in list order
            let prev = todos[..cur_idx].iter().rev().find(|t| t.status == "completed");

            // next = first pending task after current in list order
            let next = todos.get(cur_idx + 1..)
                .and_then(|rest| rest.iter().find(|t| t.status == "pending"));

            let mut parts: Vec<String> = Vec::new();

            if let Some(p) = prev {
                parts.push(format!("✓ {}", Self::trunc(&p.content, 50)));
            }

            parts.push(format!(
                "⏳ {} ({}/{})",
                Self::trunc(&current.content, 50),
                completed_count,
                total
            ));

            if let Some(n) = next {
                parts.push(format!("👉 {}", Self::trunc(&n.content, 50)));
            }

            parts.join(" → ")

        } else if completed_count == total && total > 0 {
            // All done — show last two completed as a trail
            let last_two: Vec<_> = todos.iter()
                .filter(|t| t.status == "completed")
                .rev()
                .take(2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let trail = last_two.iter()
                .map(|t| format!("✓ {}", Self::trunc(&t.content, 50)))
                .collect::<Vec<_>>()
                .join(" → ");
            format!("{}  ✦ {}/{}", trail, completed_count, total)

        } else {
            // No active task, some pending
            let first_pending = todos.iter().find(|t| t.status == "pending");
            let pending_label = first_pending
                .map(|t| format!("👉 {}", Self::trunc(&t.content, 50)))
                .unwrap_or_default();
            format!("{}/{} {}", completed_count, total, pending_label)
        };

        let mut metadata = HashMap::new();
        metadata.insert("total".to_string(), total.to_string());
        metadata.insert("completed".to_string(), completed_count.to_string());
        metadata.insert("in_progress".to_string(), in_progress.len().to_string());

        Some(SegmentData {
            primary,
            secondary: String::new(),
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::Todos
    }
}
