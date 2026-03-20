use super::{Segment, SegmentData};
use crate::config::{InputData, SegmentId};
use crate::utils::credentials;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
struct ApiUsageResponse {
    five_hour: UsagePeriod,
    seven_day: UsagePeriod,
}

#[derive(Debug, Deserialize)]
struct UsagePeriod {
    utilization: f64,
    resets_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiUsageCache {
    five_hour_utilization: f64,
    seven_day_utilization: f64,
    resets_at: Option<String>,
    cached_at: String,
}

#[derive(Default)]
pub struct UsageSegment;

impl UsageSegment {
    pub fn new() -> Self {
        Self
    }

    fn progress_bar(pct: f64, width: usize) -> String {
        let clamped = pct.clamp(0.0, 100.0);
        let filled = ((clamped / 100.0) * width as f64).round() as usize;
        let empty = width.saturating_sub(filled);
        format!("{}{}", "█".repeat(filled), "░".repeat(empty))
    }

    /// Format elapsed time from utilization for 5-hour period → "1h 15m"
    fn format_5h_elapsed(util_pct: f64) -> String {
        let elapsed_mins = (util_pct / 100.0 * 300.0).round() as u64;
        let h = elapsed_mins / 60;
        let m = elapsed_mins % 60;
        if h > 0 {
            format!("{}h {}m", h, m)
        } else {
            format!("{}m", m)
        }
    }

    /// Format elapsed time from utilization for 7-day period → "1d 22h"
    fn format_7d_elapsed(util_pct: f64) -> String {
        let elapsed_hours = (util_pct / 100.0 * 168.0).round() as u64;
        let d = elapsed_hours / 24;
        let h = elapsed_hours % 24;
        if d > 0 {
            format!("{}d {}h", d, h)
        } else {
            format!("{}h", h)
        }
    }

    fn get_circle_icon(utilization: f64) -> String {
        let percent = (utilization * 100.0) as u8;
        match percent {
            0..=12 => "\u{f0a9e}".to_string(),  // circle_slice_1
            13..=25 => "\u{f0a9f}".to_string(), // circle_slice_2
            26..=37 => "\u{f0aa0}".to_string(), // circle_slice_3
            38..=50 => "\u{f0aa1}".to_string(), // circle_slice_4
            51..=62 => "\u{f0aa2}".to_string(), // circle_slice_5
            63..=75 => "\u{f0aa3}".to_string(), // circle_slice_6
            76..=87 => "\u{f0aa4}".to_string(), // circle_slice_7
            _ => "\u{f0aa5}".to_string(),       // circle_slice_8
        }
    }

    fn get_cache_path() -> Option<std::path::PathBuf> {
        let home = dirs::home_dir()?;
        Some(
            home.join(".claude")
                .join("ccline")
                .join(".api_usage_cache.json"),
        )
    }

    fn load_cache(&self) -> Option<ApiUsageCache> {
        let cache_path = Self::get_cache_path()?;
        if !cache_path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(&cache_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn save_cache(&self, cache: &ApiUsageCache) {
        if let Some(cache_path) = Self::get_cache_path() {
            if let Some(parent) = cache_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(cache) {
                let _ = std::fs::write(&cache_path, json);
            }
        }
    }

    fn is_cache_valid(&self, cache: &ApiUsageCache, cache_duration: u64) -> bool {
        if let Ok(cached_at) = DateTime::parse_from_rfc3339(&cache.cached_at) {
            let now = Utc::now();
            let elapsed = now.signed_duration_since(cached_at.with_timezone(&Utc));
            elapsed.num_seconds() < cache_duration as i64
        } else {
            false
        }
    }

    fn get_claude_code_version() -> String {
        use std::process::Command;

        let output = Command::new("npm")
            .args(["view", "@anthropic-ai/claude-code", "version"])
            .output();

        match output {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !version.is_empty() {
                    return format!("claude-code/{}", version);
                }
            }
            _ => {}
        }

        "claude-code".to_string()
    }

    fn get_proxy_from_settings() -> Option<String> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()?;
        let settings_path = format!("{}/.claude/settings.json", home);

        let content = std::fs::read_to_string(&settings_path).ok()?;
        let settings: serde_json::Value = serde_json::from_str(&content).ok()?;

        // Try HTTPS_PROXY first, then HTTP_PROXY
        settings
            .get("env")?
            .get("HTTPS_PROXY")
            .or_else(|| settings.get("env")?.get("HTTP_PROXY"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn fetch_api_usage(
        &self,
        api_base_url: &str,
        token: &str,
        timeout_secs: u64,
    ) -> Option<ApiUsageResponse> {
        let url = format!("{}/api/oauth/usage", api_base_url);
        let user_agent = Self::get_claude_code_version();

        let agent = if let Some(proxy_url) = Self::get_proxy_from_settings() {
            if let Ok(proxy) = ureq::Proxy::new(&proxy_url) {
                ureq::Agent::config_builder()
                    .proxy(Some(proxy))
                    .build()
                    .new_agent()
            } else {
                ureq::Agent::new_with_defaults()
            }
        } else {
            ureq::Agent::new_with_defaults()
        };

        let response = agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", token))
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("User-Agent", &user_agent)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(timeout_secs)))
            .build()
            .call()
            .ok()?;

        response.into_body().read_json().ok()
    }
}

impl Segment for UsageSegment {
    fn collect(&self, _input: &InputData) -> Option<SegmentData> {
        crate::log_debug!("usage: checking for oauth token");
        let token = match credentials::get_oauth_token() {
            Some(t) => {
                crate::log_debug!("usage: oauth token found (len={})", t.len());
                t
            }
            None => {
                crate::log_debug!("usage: no oauth token found, returning None");
                return None;
            }
        };

        // Load config from file to get segment options
        let config = crate::config::Config::load().ok()?;
        let segment_config = config.segments.iter().find(|s| s.id == SegmentId::Usage);

        let api_base_url = segment_config
            .and_then(|sc| sc.options.get("api_base_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("https://api.anthropic.com");

        let cache_duration = segment_config
            .and_then(|sc| sc.options.get("cache_duration"))
            .and_then(|v| v.as_u64())
            .unwrap_or(300);

        let timeout = segment_config
            .and_then(|sc| sc.options.get("timeout"))
            .and_then(|v| v.as_u64())
            .unwrap_or(2);

        let cached_data = self.load_cache();
        let use_cached = cached_data
            .as_ref()
            .map(|cache| self.is_cache_valid(cache, cache_duration))
            .unwrap_or(false);

        let (five_hour_util, seven_day_util, resets_at) = if use_cached {
            crate::log_debug!("usage: using cached data");
            let cache = cached_data.unwrap();
            (
                cache.five_hour_utilization,
                cache.seven_day_utilization,
                cache.resets_at,
            )
        } else {
            crate::log_debug!("usage: fetching from API ({})", api_base_url);
            match self.fetch_api_usage(api_base_url, &token, timeout) {
                Some(response) => {
                    crate::log_debug!(
                        "usage: API ok 5h={} 7d={}",
                        response.five_hour.utilization,
                        response.seven_day.utilization
                    );
                    let cache = ApiUsageCache {
                        five_hour_utilization: response.five_hour.utilization,
                        seven_day_utilization: response.seven_day.utilization,
                        resets_at: response.seven_day.resets_at.clone(),
                        cached_at: Utc::now().to_rfc3339(),
                    };
                    self.save_cache(&cache);
                    (
                        response.five_hour.utilization,
                        response.seven_day.utilization,
                        response.seven_day.resets_at,
                    )
                }
                None => {
                    crate::log_debug!("usage: API failed, falling back to stale cache");
                    if let Some(cache) = cached_data {
                        (
                            cache.five_hour_utilization,
                            cache.seven_day_utilization,
                            cache.resets_at,
                        )
                    } else {
                        crate::log_debug!("usage: no cache, returning None");
                        return None;
                    }
                }
            }
        };

        let dynamic_icon = Self::get_circle_icon(seven_day_util / 100.0);
        let five_hour_percent = five_hour_util.round() as u8;
        let seven_day_percent = seven_day_util.round() as u8;

        let bar5h = Self::progress_bar(five_hour_util, 10);
        let bar7d = Self::progress_bar(seven_day_util, 10);
        let elapsed5h = Self::format_5h_elapsed(five_hour_util);
        let elapsed7d = Self::format_7d_elapsed(seven_day_util);

        let primary = format!(
            "{} {}% ({} / 5h) | {} {}% ({} / 7d)",
            bar5h, five_hour_percent, elapsed5h,
            bar7d, seven_day_percent, elapsed7d
        );
        let secondary = String::new();

        crate::log_debug!(
            "usage: 5h={:.1}% 7d={:.1}% resets_at={:?}",
            five_hour_util, seven_day_util, resets_at
        );

        let mut metadata = HashMap::new();
        metadata.insert("dynamic_icon".to_string(), dynamic_icon);
        metadata.insert(
            "five_hour_utilization".to_string(),
            five_hour_util.to_string(),
        );
        metadata.insert(
            "seven_day_utilization".to_string(),
            seven_day_util.to_string(),
        );

        Some(SegmentData {
            primary,
            secondary,
            metadata,
        })
    }

    fn id(&self) -> SegmentId {
        SegmentId::Usage
    }
}
