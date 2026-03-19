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

                // Handle TodoWrite tool
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
                            }
                        }
                    }
                }

                // Handle TaskCreate / TaskUpdate tools for ECC-style todos
                if tool_name == "TaskCreate" {
                    if let Some(input) = &block.input {
                        let id = block.id.clone().unwrap_or_else(|| format!("task_{}", latest_todos.len()));
                        let content = input
                            .get("subject")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !content.is_empty() {
                            latest_todos.push(Todo {
                                id,
                                content,
                                status: "pending".to_string(),
                                priority: "medium".to_string(),
                            });
                        }
                    }
                }

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
                            // Update todo if it matches by id or content contains task_id
                            for todo in &mut latest_todos {
                                if todo.id == task_id || todo.id.contains(&task_id) {
                                    todo.status = new_status.clone();
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        latest_todos
    }
}

impl Segment for TodosSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let todos = Self::parse_todos(&input.transcript_path);

        if todos.is_empty() {
            return None;
        }

        let total = todos.len();
        let completed_count = todos.iter().filter(|t| t.status == "completed").count();
        let in_progress: Vec<&Todo> = todos.iter().filter(|t| t.status == "in_progress").collect();

        let primary = if let Some(current) = in_progress.first() {
            let content = if current.content.len() > 25 {
                format!("{}…", &current.content[..24])
            } else {
                current.content.clone()
            };
            format!("▸ {} ({}/{})", content, completed_count, total)
        } else if completed_count == total {
            format!("✓ All done ({}/{})", completed_count, total)
        } else {
            format!("({}/{})", completed_count, total)
        };

        let mut metadata = HashMap::new();
        metadata.insert("total".to_string(), total.to_string());
        metadata.insert("completed".to_string(), completed_count.to_string());
        metadata.insert(
            "in_progress".to_string(),
            in_progress.len().to_string(),
        );

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
