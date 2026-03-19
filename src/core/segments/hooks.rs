use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId, TranscriptEntry};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RUNNING_THRESHOLD_SECS: u64 = 5;

#[derive(Debug)]
struct HookRecord {
    hook_event: String,
    hook_name: String,
    script_name: Option<String>,
    timestamp_secs: Option<u64>,
}

#[derive(Default)]
pub struct HooksSegment;

impl HooksSegment {
    pub fn new() -> Self {
        Self
    }

    /// Parse ISO 8601 timestamp (e.g. "2026-03-19T12:55:04.869Z") to unix seconds.
    fn parse_timestamp(ts: &str) -> Option<u64> {
        let ts = ts.trim_end_matches('Z');
        let ts = if let Some(dot) = ts.find('.') { &ts[..dot] } else { ts };
        let parts: Vec<&str> = ts.splitn(2, 'T').collect();
        if parts.len() != 2 { return None; }
        let date_parts: Vec<i64> = parts[0].split('-')
            .filter_map(|p| p.parse().ok())
            .collect();
        let time_parts: Vec<u64> = parts[1].split(':')
            .filter_map(|p| p.parse().ok())
            .collect();
        if date_parts.len() != 3 || time_parts.len() != 3 { return None; }

        let year = date_parts[0];
        let month = date_parts[1];
        let day = date_parts[2];

        // Days since 1970-01-01 using proleptic Gregorian calendar
        let y = if month <= 2 { year - 1 } else { year };
        let era = y / 400;
        let yoe = y - era * 400;
        let doy = (153 * (month + (if month > 2 { -3 } else { 9 })) + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days_since_epoch = era * 146097 + doe - 719468;

        if days_since_epoch < 0 { return None; }

        let seconds = days_since_epoch as u64 * 86400
            + time_parts[0] * 3600
            + time_parts[1] * 60
            + time_parts[2];
        Some(seconds)
    }

    /// Extract the script filename from a hook command string.
    fn extract_script_name(command: &str) -> Option<String> {
        for token in command.split_whitespace() {
            let token = token.trim_matches('"').trim_matches('\'');
            if (token.ends_with(".js") || token.ends_with(".sh"))
                && (token.contains('/') || token.contains('\\'))
            {
                if let Some(name) = std::path::Path::new(token).file_name() {
                    return Some(name.to_string_lossy().into_owned());
                }
            }
        }
        None
    }

    fn parse_hooks(transcript_path: &str) -> Vec<HookRecord> {
        let path = Path::new(transcript_path);
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut records: Vec<HookRecord> = Vec::new();

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

            let entry_type = match &entry.r#type {
                Some(t) => t.as_str(),
                None => continue,
            };
            if entry_type != "progress" {
                continue;
            }

            let data = match &entry.data {
                Some(d) => d,
                None => continue,
            };

            // Filter to hook_progress events only (must have at least one hook field)
            if data.hook_event.is_none() && data.hook_name.is_none() {
                continue;
            }

            let hook_event = data.hook_event.clone().unwrap_or_else(|| "Unknown".to_string());
            let hook_name = data.hook_name.clone().unwrap_or_else(|| hook_event.clone());
            let script_name = data.command.as_deref().and_then(Self::extract_script_name);
            let timestamp_secs = entry.timestamp.as_deref().and_then(Self::parse_timestamp);

            records.push(HookRecord {
                hook_event,
                hook_name,
                script_name,
                timestamp_secs,
            });
        }

        // Keep last 50 hook records
        if records.len() > 50 {
            let skip = records.len() - 50;
            records = records.into_iter().skip(skip).collect();
        }

        records
    }

    fn is_running(record: &HookRecord) -> bool {
        let Some(ts) = record.timestamp_secs else { return false };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        now.saturating_sub(ts) < RUNNING_THRESHOLD_SECS
    }
}

impl Segment for HooksSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let config = crate::config::Config::load().ok();
        let show_args = config.as_ref()
            .and_then(|c| c.segments.iter().find(|s| s.id == SegmentId::Hooks))
            .and_then(|sc| sc.options.get("show_args"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let hooks = Self::parse_hooks(&input.transcript_path);

        if hooks.is_empty() {
            return None;
        }

        let running: Vec<&HookRecord> = hooks.iter().filter(|h| Self::is_running(h)).collect();
        let completed: Vec<&HookRecord> = hooks.iter().filter(|h| !Self::is_running(h)).collect();

        let mut parts: Vec<String> = Vec::new();

        // Running hooks first (rare — usually only 1 at a time)
        for hook in running.iter().take(2) {
            let label = if show_args {
                if let Some(ref script) = hook.script_name {
                    format!("◐ {} {}", hook.hook_name, script)
                } else {
                    format!("◐ {}", hook.hook_name)
                }
            } else {
                format!("◐ {}", hook.hook_name)
            };
            parts.push(label);
        }

        // Recent completed hooks
        let max_completed = 4usize.saturating_sub(parts.len());
        let recent: Vec<_> = completed.iter()
            .rev()
            .take(max_completed)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        for hook in recent {
            let label = if show_args {
                if let Some(ref script) = hook.script_name {
                    format!("✓ {} {}", hook.hook_name, script)
                } else {
                    format!("✓ {}", hook.hook_name)
                }
            } else {
                format!("✓ {}", hook.hook_name)
            };
            parts.push(label);
        }

        if parts.is_empty() {
            return None;
        }

        let primary = parts.join(" | ");

        // Secondary: counts grouped by hookEvent
        let mut event_counts: HashMap<String, u32> = HashMap::new();
        for hook in &hooks {
            *event_counts.entry(hook.hook_event.clone()).or_insert(0) += 1;
        }
        let mut event_vec: Vec<(String, u32)> = event_counts.into_iter().collect();
        event_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let secondary = event_vec.iter()
            .map(|(event, count)| {
                let short = match event.as_str() {
                    "PostToolUse" => "Post",
                    "PreToolUse"  => "Pre",
                    "SessionStart" => "Start",
                    "Stop"        => "Stop",
                    other         => other,
                };
                format!("{}×{}", short, count)
            })
            .collect::<Vec<_>>()
            .join("  ");

        let mut metadata = HashMap::new();
        metadata.insert("running".to_string(), running.len().to_string());
        metadata.insert("total".to_string(), hooks.len().to_string());

        Some(SegmentData {
            primary,
            secondary,
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::Hooks
    }
}
