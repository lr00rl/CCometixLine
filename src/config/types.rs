use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Main config structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub style: StyleConfig,
    pub segments: Vec<SegmentConfig>,
    pub theme: String,
}

// Default implementation moved to ui/themes/presets.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleConfig {
    pub mode: StyleMode,
    pub separator: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleMode {
    Plain,
    NerdFont,
    Powerline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentConfig {
    pub id: SegmentId,
    pub enabled: bool,
    #[serde(default)]
    pub line: u8,
    pub icon: IconConfig,
    pub colors: ColorConfig,
    pub styles: TextStyleConfig,
    pub options: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconConfig {
    pub plain: String,
    pub nerd_font: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorConfig {
    pub icon: Option<AnsiColor>,
    pub text: Option<AnsiColor>,
    pub background: Option<AnsiColor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TextStyleConfig {
    pub text_bold: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnsiColor {
    Color16 { c16: u8 },
    Color256 { c256: u8 },
    Rgb { r: u8, g: u8, b: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentId {
    Model,
    Directory,
    Git,
    ContextWindow,
    Usage,
    Cost,
    Session,
    OutputStyle,
    Update,
    Tools,
    Agents,
    Todos,
    Environment,
    SessionName,
    Skills,
    Hooks,
}

// Legacy compatibility structure
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SegmentsConfig {
    pub directory: bool,
    pub git: bool,
    pub model: bool,
    // pub usage: bool,
}

// Data structures compatible with existing main.rs
#[derive(Deserialize, Default)]
pub struct Model {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Deserialize, Default)]
pub struct Workspace {
    #[serde(default)]
    pub current_dir: String,
}

#[derive(Deserialize)]
pub struct Cost {
    pub total_cost_usd: Option<f64>,
    pub total_duration_ms: Option<u64>,
    pub total_api_duration_ms: Option<u64>,
    pub total_lines_added: Option<u32>,
    pub total_lines_removed: Option<u32>,
}

#[derive(Deserialize)]
pub struct OutputStyle {
    pub name: String,
}

/// Per-request token breakdown provided directly by the host agent (pi sends
/// this inside `context_window.current_usage`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CurrentUsage {
    #[serde(default)]
    pub input_tokens: Option<u32>,
    #[serde(default)]
    pub output_tokens: Option<u32>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,
}

impl CurrentUsage {
    /// Tokens currently occupying the context window.
    pub fn context_tokens(&self) -> u32 {
        self.input_tokens.unwrap_or(0)
            + self.output_tokens.unwrap_or(0)
            + self.cache_creation_input_tokens.unwrap_or(0)
            + self.cache_read_input_tokens.unwrap_or(0)
    }
}

/// Context window info provided directly by the host agent, bypassing
/// transcript parsing and models.toml limits when present.
///
/// Field aliases cover the two known dialects:
///   - pi (`pi-statusline`): `context_window` with `context_window_size`,
///     `used_percentage` and a `current_usage` breakdown
///   - kimi-code statusline proposal (PR #2043): `context` with
///     `used_tokens` / `max_tokens` / `percent`
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ContextWindowInput {
    #[serde(default, alias = "max_tokens")]
    pub context_window_size: Option<u32>,
    #[serde(default, alias = "percent")]
    pub used_percentage: Option<f64>,
    #[serde(default)]
    pub used_tokens: Option<u32>,
    #[serde(default)]
    pub total_input_tokens: Option<u32>,
    #[serde(default)]
    pub total_output_tokens: Option<u32>,
    #[serde(default)]
    pub current_usage: Option<CurrentUsage>,
}

/// pi-specific extension block (`pi-statusline` package).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PiExtension {
    /// Path to pi's full native session JSONL (richer than the minimal
    /// statusline transcript pi writes to `transcript_path`).
    #[serde(default)]
    pub session_file: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct InputData {
    #[serde(default)]
    pub model: Model,
    #[serde(default)]
    pub workspace: Workspace,
    #[serde(default)]
    pub transcript_path: String,
    #[serde(default)]
    pub cost: Option<Cost>,
    #[serde(default)]
    pub output_style: Option<OutputStyle>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default, alias = "context")]
    pub context_window: Option<ContextWindowInput>,
    #[serde(default)]
    pub pi: Option<PiExtension>,
}

impl ContextWindowInput {
    /// Best available count of tokens currently in the context window.
    /// Priority: current_usage breakdown > used_tokens > total input+output.
    pub fn used_context_tokens(&self) -> Option<u32> {
        if let Some(usage) = &self.current_usage {
            let t = usage.context_tokens();
            if t > 0 {
                return Some(t);
            }
        }
        if let Some(t) = self.used_tokens {
            if t > 0 {
                return Some(t);
            }
        }
        match (self.total_input_tokens, self.total_output_tokens) {
            (None, None) => None,
            (i, o) => {
                let t = i.unwrap_or(0) + o.unwrap_or(0);
                if t > 0 {
                    Some(t)
                } else {
                    None
                }
            }
        }
    }
}

// OpenAI-style nested token details
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<u32>,
    #[serde(default)]
    pub audio_tokens: Option<u32>,
}

// Raw usage data from different LLM providers (flexible parsing)
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RawUsage {
    // Anthropic-style input tokens
    #[serde(default)]
    pub input_tokens: Option<u32>,

    // OpenAI-style input tokens (separate field to handle both formats)
    #[serde(default)]
    pub prompt_tokens: Option<u32>,

    // Anthropic-style output tokens
    #[serde(default)]
    pub output_tokens: Option<u32>,

    // OpenAI-style output tokens (separate field to handle both formats)
    #[serde(default)]
    pub completion_tokens: Option<u32>,

    // Total tokens (some providers only provide this)
    #[serde(default)]
    pub total_tokens: Option<u32>,

    // Anthropic-style cache fields
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u32>,

    #[serde(default)]
    pub cache_read_input_tokens: Option<u32>,

    // OpenAI-style cache fields (separate fields to handle both formats)
    #[serde(default)]
    pub cache_creation_prompt_tokens: Option<u32>,

    #[serde(default)]
    pub cache_read_prompt_tokens: Option<u32>,

    #[serde(default)]
    pub cached_tokens: Option<u32>,

    // OpenAI-style nested details
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,

    // Completion token details (OpenAI)
    #[serde(default)]
    pub completion_tokens_details: Option<HashMap<String, u32>>,

    // Catch unknown fields for future compatibility and debugging
    #[serde(flatten, skip_serializing)]
    pub extra: HashMap<String, serde_json::Value>,
}

// Normalized internal representation after processing
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct NormalizedUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    pub cache_creation_input_tokens: u32,
    pub cache_read_input_tokens: u32,

    // Metadata for debugging and analysis
    pub calculation_source: String,
    pub raw_data_available: Vec<String>,
}

impl NormalizedUsage {
    /// Get tokens that count toward context window
    /// This includes all tokens that consume context window space
    /// Output tokens from this turn will become input tokens in the next turn
    pub fn context_tokens(&self) -> u32 {
        self.input_tokens
            + self.cache_creation_input_tokens
            + self.cache_read_input_tokens
            + self.output_tokens
    }

    /// Get total tokens for cost calculation
    /// Priority: use total_tokens if available, otherwise sum all components
    pub fn total_for_cost(&self) -> u32 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens
                + self.output_tokens
                + self.cache_creation_input_tokens
                + self.cache_read_input_tokens
        }
    }

    /// Get the most appropriate token count for general display
    /// For OpenAI format: use total_tokens directly
    /// For Anthropic format: use context_tokens (input + cache)
    pub fn display_tokens(&self) -> u32 {
        // For Claude/Anthropic format: prefer input-related tokens for context window display
        let context = self.context_tokens();
        if context > 0 {
            return context;
        }

        // For OpenAI format: use total_tokens when no input breakdown available
        if self.total_tokens > 0 {
            return self.total_tokens;
        }

        // Fallback to any available tokens
        self.input_tokens.max(self.output_tokens)
    }
}

impl Config {
    /// Check if current config matches the specified theme preset
    pub fn matches_theme(&self, theme_name: &str) -> bool {
        let theme_preset = crate::ui::themes::ThemePresets::get_theme(theme_name);

        // Compare style config
        if self.style.mode != theme_preset.style.mode
            || self.style.separator != theme_preset.style.separator
        {
            return false;
        }

        // Compare segments count and order
        if self.segments.len() != theme_preset.segments.len() {
            return false;
        }

        // Compare each segment config
        for (current, preset) in self.segments.iter().zip(theme_preset.segments.iter()) {
            if !self.segment_matches(current, preset) {
                return false;
            }
        }

        true
    }

    /// Check if current config has been modified from the selected theme
    pub fn is_modified_from_theme(&self) -> bool {
        !self.matches_theme(&self.theme)
    }

    /// Compare two segment configs for equality
    fn segment_matches(&self, current: &SegmentConfig, preset: &SegmentConfig) -> bool {
        current.id == preset.id
            && current.enabled == preset.enabled
            && current.icon.plain == preset.icon.plain
            && current.icon.nerd_font == preset.icon.nerd_font
            && self.color_matches(&current.colors.icon, &preset.colors.icon)
            && self.color_matches(&current.colors.text, &preset.colors.text)
            && self.color_matches(&current.colors.background, &preset.colors.background)
            && current.styles.text_bold == preset.styles.text_bold
            && current.options == preset.options
    }

    /// Compare two optional colors for equality
    fn color_matches(&self, current: &Option<AnsiColor>, preset: &Option<AnsiColor>) -> bool {
        match (current, preset) {
            (None, None) => true,
            (Some(c1), Some(c2)) => c1 == c2,
            _ => false,
        }
    }
}

impl PartialEq for AnsiColor {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (AnsiColor::Color16 { c16: a }, AnsiColor::Color16 { c16: b }) => a == b,
            (AnsiColor::Color256 { c256: a }, AnsiColor::Color256 { c256: b }) => a == b,
            (
                AnsiColor::Rgb {
                    r: r1,
                    g: g1,
                    b: b1,
                },
                AnsiColor::Rgb {
                    r: r2,
                    g: g2,
                    b: b2,
                },
            ) => r1 == r2 && g1 == g2 && b1 == b2,
            _ => false,
        }
    }
}

impl RawUsage {
    /// Convert raw usage data to normalized format with intelligent token inference
    pub fn normalize(self) -> NormalizedUsage {
        let mut result = NormalizedUsage::default();
        let mut sources = Vec::new();

        // Collect available raw data fields and merge tokens with Anthropic priority
        let mut available_fields = Vec::new();

        // Merge input tokens (priority: input_tokens > prompt_tokens)
        let input = self.input_tokens.or(self.prompt_tokens).unwrap_or(0);
        if input > 0 {
            available_fields.push("input_tokens".to_string());
        }

        // Merge output tokens (priority: output_tokens > completion_tokens)
        let output = self.output_tokens.or(self.completion_tokens).unwrap_or(0);
        if output > 0 {
            available_fields.push("output_tokens".to_string());
        }

        let total = self.total_tokens.unwrap_or(0);
        if total > 0 {
            available_fields.push("total_tokens".to_string());
        }

        // Merge cache creation tokens (priority: Anthropic > OpenAI)
        let cache_creation = self
            .cache_creation_input_tokens
            .or(self.cache_creation_prompt_tokens)
            .unwrap_or(0);
        if cache_creation > 0 {
            available_fields.push("cache_creation".to_string());
        }

        // Merge cache read tokens (priority: Anthropic > OpenAI > nested format)
        let cache_read = self
            .cache_read_input_tokens
            .or(self.cache_read_prompt_tokens)
            .or(self.cached_tokens)
            .or_else(|| {
                // Fallback to OpenAI nested format
                self.prompt_tokens_details
                    .as_ref()
                    .and_then(|d| d.cached_tokens)
            })
            .unwrap_or(0);
        if cache_read > 0 {
            available_fields.push("cache_read".to_string());
        }

        result.raw_data_available = available_fields;

        // Use merged cache values (already calculated above with Anthropic priority)

        // Token calculation logic - prioritize total_tokens for OpenAI format
        let total_value = if total > 0 {
            sources.push("total_tokens_direct".to_string());
            total
        } else if input > 0 || output > 0 || cache_read > 0 || cache_creation > 0 {
            let calculated = input + output + cache_read + cache_creation;
            sources.push("total_from_components".to_string());
            calculated
        } else {
            0
        };

        // Assignment
        result.input_tokens = input;
        result.output_tokens = output;
        result.total_tokens = total_value;
        result.cache_creation_input_tokens = cache_creation;
        result.cache_read_input_tokens = cache_read;
        result.calculation_source = sources.join("+");

        result
    }
}

// Legacy alias for backward compatibility
pub type Usage = RawUsage;

#[cfg(test)]
mod input_tests {
    use super::*;

    /// Real payload shape emitted by pi's `pi-statusline` package.
    #[test]
    fn parses_pi_payload() {
        let raw = r#"{"cwd":"/Users/x/.pi","session_id":"019f82c0","transcript_path":"/Users/x/.pi/agent/statusline-transcripts/019f82c0.jsonl","model":{"id":"k3","display_name":"Kimi K3"},"workspace":{"current_dir":"/Users/x/.pi","project_dir":"/Users/x/.pi"},"version":null,"output_style":{"name":"default"},"cost":{"total_cost_usd":0.178,"total_duration_ms":4741596,"total_api_duration_ms":null,"total_lines_added":null,"total_lines_removed":null},"context_window":{"total_input_tokens":22078,"total_output_tokens":4586,"context_window_size":1048576,"used_percentage":1,"remaining_percentage":99,"current_usage":{"input_tokens":441,"output_tokens":466,"cache_creation_input_tokens":0,"cache_read_input_tokens":10496}},"exceeds_200k_tokens":false,"rate_limits":null,"vim":null,"agent":null,"worktree":null,"pi":{"session_file":"/Users/x/.pi/agent/sessions/x/2026.jsonl","refreshed_at_ms":1784615197794}}"#;
        let input: InputData = serde_json::from_str(raw).expect("pi payload should parse");
        assert_eq!(input.model.id, "k3");
        assert_eq!(input.session_id.as_deref(), Some("019f82c0"));
        let cw = input.context_window.expect("context_window present");
        assert_eq!(cw.context_window_size, Some(1048576));
        assert_eq!(cw.used_context_tokens(), Some(441 + 466 + 10496));
        let pi = input.pi.expect("pi extension present");
        assert!(pi.session_file.unwrap().ends_with("2026.jsonl"));
    }

    /// Claude Code's own payload has no context_window block — everything
    /// stays optional and transcript parsing remains the fallback.
    #[test]
    fn parses_claude_code_payload() {
        let raw = r#"{"model":{"id":"claude-sonnet-5","display_name":"Sonnet 5"},"workspace":{"current_dir":"/w"},"transcript_path":"/t.jsonl","cost":{"total_cost_usd":1.0,"total_duration_ms":1,"total_api_duration_ms":1,"total_lines_added":0,"total_lines_removed":0},"output_style":{"name":"default"}}"#;
        let input: InputData = serde_json::from_str(raw).expect("claude payload should parse");
        assert!(input.context_window.is_none());
        assert_eq!(input.transcript_path, "/t.jsonl");
    }

    /// kimi-code statusline proposal (PR #2043) dialect: `context` block with
    /// used_tokens / max_tokens / percent and no transcript_path.
    #[test]
    fn parses_kimi_context_dialect() {
        let raw = r#"{"session_id":"s1","model":{"id":"kimi-k3","display_name":"Kimi K3"},"workspace":{"current_dir":"/w"},"context":{"used_tokens":50000,"max_tokens":1048576,"percent":4.8}}"#;
        let input: InputData = serde_json::from_str(raw).expect("kimi payload should parse");
        assert_eq!(input.transcript_path, "");
        let cw = input.context_window.expect("context alias mapped");
        assert_eq!(cw.context_window_size, Some(1048576));
        assert_eq!(cw.used_context_tokens(), Some(50000));
    }
}

/// A single content block within a message (tool_use, tool_result, text, etc.)
#[derive(Debug, Clone, Deserialize)]
pub struct ContentBlock {
    pub r#type: String,
    pub id: Option<String>,
    #[serde(rename = "tool_use_id")]
    pub tool_use_id: Option<String>,
    pub name: Option<String>,
    pub input: Option<serde_json::Value>,
    pub content: Option<serde_json::Value>, // tool_result content (string or array)
}

#[derive(Debug, Clone, Deserialize)]
pub struct HookProgressData {
    #[serde(rename = "hookEvent")]
    pub hook_event: Option<String>,
    #[serde(rename = "hookName")]
    pub hook_name: Option<String>,
    pub command: Option<String>,
}

#[derive(Deserialize)]
pub struct Message {
    pub usage: Option<Usage>,
    pub content: Option<Vec<ContentBlock>>,
    pub role: Option<String>,
}

#[derive(Deserialize)]
pub struct TranscriptEntry {
    pub r#type: Option<String>,
    pub message: Option<Message>,
    #[serde(rename = "leafUuid")]
    pub leaf_uuid: Option<String>,
    pub uuid: Option<String>,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    pub summary: Option<String>,
    pub title: Option<String>,
    pub timestamp: Option<String>,
    pub data: Option<HookProgressData>,
}
