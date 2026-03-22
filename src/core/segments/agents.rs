use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId, TranscriptEntry};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug)]
struct AgentRecord {
    #[allow(dead_code)]
    tool_use_id: String,
    subagent_type: String,
    description: String,
    model: Option<String>,
    start_time: u64, // unix ms, approximated by order
    completed: bool,
    duration_s: Option<u64>,
}

#[derive(Default)]
pub struct AgentsSegment;

impl AgentsSegment {
    pub fn new() -> Self {
        Self
    }

    fn parse_agents(transcript_path: &str) -> Vec<AgentRecord> {
        let path = Path::new(transcript_path);
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut agent_map: HashMap<String, AgentRecord> = HashMap::new();
        let mut line_counter: u64 = 0;

        for line in reader.lines() {
            line_counter += 1;
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
                    "tool_use" if matches!(block.name.as_deref(), Some("Task") | Some("Agent")) => {
                        if let Some(id) = &block.id {
                            let input = block.input.as_ref();
                            let subagent_type = input
                                .and_then(|v| v.get("subagent_type"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let description = input
                                .and_then(|v| v.get("description"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let model = input
                                .and_then(|v| v.get("model"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            agent_map.insert(
                                id.clone(),
                                AgentRecord {
                                    tool_use_id: id.clone(),
                                    subagent_type,
                                    description,
                                    model,
                                    start_time: line_counter,
                                    completed: false,
                                    duration_s: None,
                                },
                            );
                        }
                    }
                    "tool_result" => {
                        if let Some(tool_use_id) = &block.tool_use_id {
                            if let Some(record) = agent_map.get_mut(tool_use_id) {
                                record.completed = true;
                                // Approximate duration (can't get real time without timestamps)
                                let elapsed = line_counter.saturating_sub(record.start_time);
                                record.duration_s = Some(elapsed);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut records: Vec<AgentRecord> = agent_map.into_values().collect();
        records.sort_by(|a, b| a.start_time.cmp(&b.start_time));
        records
    }

    fn format_duration(seconds: u64) -> String {
        if seconds < 60 {
            format!("{}s", seconds)
        } else {
            let m = seconds / 60;
            let s = seconds % 60;
            if s == 0 {
                format!("{}m", m)
            } else {
                format!("{}m{}s", m, s)
            }
        }
    }

    fn shorten_model(model: &str) -> String {
        if model.contains("sonnet") {
            "sonnet".to_string()
        } else if model.contains("opus") {
            "opus".to_string()
        } else if model.contains("haiku") {
            "haiku".to_string()
        } else if model.len() > 10 {
            format!("{}…", &model[..9])
        } else {
            model.to_string()
        }
    }
}

impl Segment for AgentsSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        crate::log_debug!("agents: reading transcript {:?}", input.transcript_path);
        let agents = Self::parse_agents(&input.transcript_path);

        if agents.is_empty() {
            crate::log_debug!("agents: no tool_use with name=\"Task\"/\"Agent\" found in transcript, returning None");
            return None;
        }

        let running: Vec<&AgentRecord> = agents.iter().filter(|a| !a.completed).collect();
        let completed: Vec<&AgentRecord> = agents.iter().filter(|a| a.completed).collect();
        crate::log_debug!(
            "agents: total={} running={} completed={}",
            agents.len(), running.len(), completed.len()
        );
        for agent in &agents {
            crate::log_debug!(
                "agents:   id={} type={:?} model={:?} completed={} desc={:?}",
                agent.tool_use_id, agent.subagent_type, agent.model, agent.completed, agent.description
            );
        }

        let mut parts: Vec<String> = Vec::new();

        let format_agent = |agent: &AgentRecord, icon: &str| -> String {
            let mut s = format!("{} {}", icon, agent.subagent_type);
            if let Some(ref model) = agent.model {
                s.push_str(&format!(" [{}]", Self::shorten_model(model)));
            }
            if let Some(dur) = agent.duration_s {
                if agent.completed {
                    s.push_str(&format!(" {}",  Self::format_duration(dur)));
                }
            }
            if !agent.description.is_empty() {
                let desc = if agent.description.len() > 30 {
                    format!("{}… ", &agent.description[..29])
                } else {
                    agent.description.clone()
                };
                s.push_str(&format!(" · {}", desc));
            }
            s
        };

        // Show running agents first (up to 2)
        for agent in running.iter().take(2) {
            parts.push(format_agent(agent, "◐"));
        }

        // Show last completed agents (up to 2)
        for agent in completed.iter().rev().take(2).collect::<Vec<_>>().into_iter().rev() {
            parts.push(format_agent(agent, "✓"));
        }

        if parts.is_empty() {
            return None;
        }

        let primary = parts.join("  ");

        // Secondary: summary line
        let total = agents.len();
        let secondary = if total > 0 {
            let mut summary = format!("=> Agent×{}", total);
            if !running.is_empty() || !completed.is_empty() {
                let mut status_parts: Vec<String> = Vec::new();
                if !completed.is_empty() {
                    status_parts.push(format!("{} done", completed.len()));
                }
                if !running.is_empty() {
                    status_parts.push(format!("{} running", running.len()));
                }
                summary.push_str(&format!(" ({})", status_parts.join(", ")));
            }
            summary
        } else {
            String::new()
        };

        let mut metadata = HashMap::new();
        metadata.insert("running".to_string(), running.len().to_string());
        metadata.insert("completed".to_string(), completed.len().to_string());
        metadata.insert("total".to_string(), total.to_string());

        Some(SegmentData {
            primary,
            secondary,
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::Agents
    }
}
