use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId, TranscriptEntry};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug)]
struct SkillRecord {
    id: String,
    skill_name: String,
    #[allow(dead_code)]
    args: Option<String>,
    completed: bool,
}

#[derive(Default)]
pub struct SkillsSegment;

impl SkillsSegment {
    pub fn new() -> Self {
        Self
    }

    fn parse_skills(transcript_path: &str) -> Vec<SkillRecord> {
        let path = Path::new(transcript_path);
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut skill_map: HashMap<String, SkillRecord> = HashMap::new();

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
                            if name != "Skill" {
                                continue;
                            }
                            let skill_name = block.input.as_ref()
                                .and_then(|v| v.get("skill"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let args = block.input.as_ref()
                                .and_then(|v| v.get("args"))
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            skill_map.insert(
                                id.clone(),
                                SkillRecord {
                                    id: id.clone(),
                                    skill_name,
                                    args,
                                    completed: false,
                                },
                            );
                        }
                    }
                    "tool_result" => {
                        if let Some(tool_use_id) = &block.tool_use_id {
                            if let Some(record) = skill_map.get_mut(tool_use_id) {
                                record.completed = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut records: Vec<SkillRecord> = skill_map.into_values().collect();
        records.sort_by(|a, b| a.id.cmp(&b.id));
        records
    }

    fn shorten_skill_name(name: &str) -> String {
        // Strip namespace prefix (e.g., "superpowers:brainstorming" → "brainstorming")
        if let Some(pos) = name.rfind(':') {
            name[pos + 1..].to_string()
        } else {
            name.to_string()
        }
    }
}

impl Segment for SkillsSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let skills = Self::parse_skills(&input.transcript_path);

        if skills.is_empty() {
            return None;
        }

        let running: Vec<&SkillRecord> = skills.iter().filter(|s| !s.completed).collect();
        let completed: Vec<&SkillRecord> = skills.iter().filter(|s| s.completed).collect();

        let mut parts: Vec<String> = Vec::new();

        // Running skills first
        for skill in running.iter().take(3) {
            let short = Self::shorten_skill_name(&skill.skill_name);
            parts.push(format!("◐ {}", short));
        }

        // Recent completed (most recent last, up to 6 total)
        let max_completed = 6usize.saturating_sub(parts.len());
        let recent: Vec<_> = completed.iter()
            .rev()
            .take(max_completed)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        for skill in recent {
            let short = Self::shorten_skill_name(&skill.skill_name);
            parts.push(format!("✓ {}", short));
        }

        if parts.is_empty() {
            return None;
        }

        let primary = parts.join(" | ");
        let total = skills.len();
        let secondary = format!("{} skill{}", total, if total == 1 { "" } else { "s" });

        let mut metadata = HashMap::new();
        metadata.insert("running".to_string(), running.len().to_string());
        metadata.insert("completed".to_string(), completed.len().to_string());

        Some(SegmentData {
            primary,
            secondary,
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::Skills
    }
}
