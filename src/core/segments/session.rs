use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId, TranscriptEntry};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct SpeedCache {
    output_tokens: u32,
    timestamp_ms: u64,
}

#[derive(Default)]
pub struct SessionSegment;

impl SessionSegment {
    pub fn new() -> Self {
        Self
    }

    fn get_speed_cache_path() -> Option<std::path::PathBuf> {
        dirs::home_dir().map(|h| h.join(".claude").join("ccline").join(".speed_cache.json"))
    }

    fn read_speed_cache() -> SpeedCache {
        let path = match Self::get_speed_cache_path() {
            Some(p) => p,
            None => return SpeedCache::default(),
        };
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return SpeedCache::default(),
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    fn write_speed_cache(cache: &SpeedCache) {
        let path = match Self::get_speed_cache_path() {
            Some(p) => p,
            None => return,
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(cache) {
            let _ = fs::write(&path, json);
        }
    }

    fn get_current_output_tokens(transcript_path: &str) -> u32 {
        let path = Path::new(transcript_path);
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return 0,
        };

        let reader = BufReader::new(file);
        let mut total_output: u32 = 0;

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

            if entry.r#type.as_deref() == Some("assistant") {
                if let Some(message) = &entry.message {
                    if let Some(raw_usage) = &message.usage {
                        let normalized = raw_usage.clone().normalize();
                        total_output += normalized.output_tokens;
                    }
                }
            }
        }

        total_output
    }

    fn calculate_speed(transcript_path: &str, show_speed: bool) -> Option<f64> {
        if !show_speed {
            return None;
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let current_tokens = Self::get_current_output_tokens(transcript_path);
        let cache = Self::read_speed_cache();

        let speed = if cache.output_tokens > 0 && cache.timestamp_ms > 0 && current_tokens > cache.output_tokens {
            let elapsed_s = (now_ms.saturating_sub(cache.timestamp_ms)) as f64 / 1000.0;
            if elapsed_s > 0.5 {
                let delta_tokens = (current_tokens - cache.output_tokens) as f64;
                Some(delta_tokens / elapsed_s)
            } else {
                None
            }
        } else {
            None
        };

        // Update cache
        Self::write_speed_cache(&SpeedCache {
            output_tokens: current_tokens,
            timestamp_ms: now_ms,
        });

        speed
    }

    fn format_duration(ms: u64) -> String {
        if ms < 1000 {
            format!("{}ms", ms)
        } else if ms < 60_000 {
            let seconds = ms / 1000;
            format!("{}s", seconds)
        } else if ms < 3_600_000 {
            let minutes = ms / 60_000;
            let seconds = (ms % 60_000) / 1000;
            if seconds == 0 {
                format!("{}m", minutes)
            } else {
                format!("{}m{}s", minutes, seconds)
            }
        } else {
            let hours = ms / 3_600_000;
            let minutes = (ms % 3_600_000) / 60_000;
            if minutes == 0 {
                format!("{}h", hours)
            } else {
                format!("{}h{}m", hours, minutes)
            }
        }
    }
}

impl Segment for SessionSegment {
    fn collect(&self, input: &InputData) -> Option<SegmentData> {
        let cost_data = input.cost.as_ref()?;

        // Primary display: total duration
        let primary = if let Some(duration) = cost_data.total_duration_ms {
            Self::format_duration(duration)
        } else {
            return None;
        };

        // Secondary display: line changes if available (green for +, red for -)
        let mut secondary_parts: Vec<String> = Vec::new();

        match (cost_data.total_lines_added, cost_data.total_lines_removed) {
            (Some(added), Some(removed)) if added > 0 || removed > 0 => {
                secondary_parts.push(format!("\x1b[32m+{}\x1b[0m \x1b[31m-{}\x1b[0m", added, removed));
            }
            (Some(added), None) if added > 0 => {
                secondary_parts.push(format!("\x1b[32m+{}\x1b[0m", added));
            }
            (None, Some(removed)) if removed > 0 => {
                secondary_parts.push(format!("\x1b[31m-{}\x1b[0m", removed));
            }
            _ => {}
        }

        // Optionally show token speed
        // show_speed option is read from segment options via InputData options field
        // For now we check the transcript_path to compute speed
        // The option would come from segment_config.options but Segment::collect only gets InputData
        // We always compute speed and include it; the segment config controls display
        if let Some(speed) = Self::calculate_speed(&input.transcript_path, true) {
            secondary_parts.push(format!("{:.1} tok/s", speed));
        }

        let secondary = secondary_parts.join(" · ");

        let mut metadata = HashMap::new();
        if let Some(duration) = cost_data.total_duration_ms {
            metadata.insert("duration_ms".to_string(), duration.to_string());
        }
        if let Some(api_duration) = cost_data.total_api_duration_ms {
            metadata.insert("api_duration_ms".to_string(), api_duration.to_string());
        }
        if let Some(added) = cost_data.total_lines_added {
            metadata.insert("lines_added".to_string(), added.to_string());
        }
        if let Some(removed) = cost_data.total_lines_removed {
            metadata.insert("lines_removed".to_string(), removed.to_string());
        }

        Some(SegmentData {
            primary,
            secondary,
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::Session
    }
}
