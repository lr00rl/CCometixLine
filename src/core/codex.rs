//! OpenAI Codex CLI adapter.
//!
//! Codex has no external statusline command support (its `[tui] status_line`
//! only accepts built-in item ids, see openai/codex#17827), so instead of
//! receiving JSON on stdin we locate the active rollout session file under
//! `$CODEX_HOME/sessions` (default `~/.codex/sessions`) and synthesize an
//! `InputData` from it. Intended for tmux status bars / sidecar panes:
//!
//! ```text
//! ccline --codex            # auto-detect newest session (prefers cwd match)
//! ccline --codex-session /path/to/rollout-....jsonl
//! ```
//!
//! Rollout format (one JSON object per line):
//!   - line 1: `{"type":"session_meta","payload":{"id","cwd","thread_source",
//!     "source":{"subagent":{...}}, ...}}`
//!   - `{"type":"turn_context","payload":{"cwd","model",...}}`
//!   - `{"type":"event_msg","payload":{"type":"token_count","info":{
//!     "total_token_usage":{...},"last_token_usage":{"input_tokens",
//!     "cached_input_tokens","cache_write_input_tokens","output_tokens",...},
//!     "model_context_window"}, "rate_limits":{...}}}`

use crate::config::{ContextWindowInput, CurrentUsage, InputData, Model, Workspace};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

/// Max number of newest rollout files to probe when auto-detecting.
const MAX_CANDIDATES: usize = 50;

pub fn build_input(explicit_session: Option<&str>) -> Option<InputData> {
    let session_path = match explicit_session {
        Some(p) => PathBuf::from(shellexpand_tilde(p)),
        None => find_active_session()?,
    };
    crate::log_debug!("codex: using rollout {:?}", session_path);
    parse_rollout(&session_path)
}

fn shellexpand_tilde(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    p.to_string()
}

fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

/// Collect rollout-*.jsonl files under sessions/YYYY/MM/DD, newest first.
fn collect_rollouts(sessions_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut stack = vec![sessions_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
                .unwrap_or(false)
            {
                let mtime = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                files.push((mtime, path));
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().map(|(_, p)| p).collect()
}

/// Read only the first line of a rollout and decide whether it is a
/// top-level (non-subagent) session; returns its recorded cwd.
fn probe_session_meta(path: &Path) -> Option<(bool, String)> {
    let file = fs::File::open(path).ok()?;
    let mut first_line = String::new();
    BufReader::new(file).read_line(&mut first_line).ok()?;
    let v: Value = serde_json::from_str(first_line.trim()).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
        return None;
    }
    let payload = v.get("payload")?;
    let is_subagent = payload
        .get("thread_source")
        .and_then(|s| s.as_str())
        .map(|s| s == "subagent")
        .unwrap_or(false)
        || payload
            .get("source")
            .map(|s| s.get("subagent").is_some())
            .unwrap_or(false);
    let cwd = payload
        .get("cwd")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    Some((!is_subagent, cwd))
}

/// Pick the newest top-level session, preferring one whose cwd matches the
/// current working directory (so per-project tmux panes show their own
/// session).
fn find_active_session() -> Option<PathBuf> {
    let sessions_dir = codex_home().join("sessions");
    let rollouts = collect_rollouts(&sessions_dir);
    let current_dir = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut newest_toplevel: Option<PathBuf> = None;
    for path in rollouts.into_iter().take(MAX_CANDIDATES) {
        let Some((is_toplevel, cwd)) = probe_session_meta(&path) else {
            continue;
        };
        if !is_toplevel {
            continue;
        }
        if !current_dir.is_empty() && cwd == current_dir {
            return Some(path);
        }
        if newest_toplevel.is_none() {
            newest_toplevel = Some(path);
        }
    }
    newest_toplevel
}

/// Prettify codex model ids for display: "gpt-5.4" -> "GPT-5.4",
/// "gpt-5.5-codex" -> "GPT-5.5 Codex".
fn prettify_model(id: &str) -> String {
    let mut name = id.to_string();
    if let Some(rest) = name.strip_prefix("gpt") {
        name = format!("GPT{}", rest);
    }
    name = name.replace("-codex", " Codex");
    name
}

fn parse_rollout(path: &Path) -> Option<InputData> {
    let mut raw = String::new();
    fs::File::open(path)
        .ok()?
        .read_to_string(&mut raw)
        .ok()?;

    let mut session_id = None;
    let mut meta_cwd = String::new();
    if let Some(first) = raw.lines().next() {
        if let Ok(v) = serde_json::from_str::<Value>(first.trim()) {
            if let Some(payload) = v.get("payload") {
                session_id = payload
                    .get("id")
                    .or_else(|| payload.get("session_id"))
                    .and_then(|s| s.as_str())
                    .map(String::from);
                meta_cwd = payload
                    .get("cwd")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
            }
        }
    }

    // Scan backwards for the latest turn_context (model, cwd) and the latest
    // token_count event (usage + context window size).
    let mut model_id = String::new();
    let mut turn_cwd = String::new();
    let mut token_info: Option<Value> = None;
    for line in raw.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let payload = v.get("payload");
        match typ {
            "turn_context" if model_id.is_empty() => {
                if let Some(p) = payload {
                    model_id = p
                        .get("model")
                        .and_then(|m| m.as_str())
                        .unwrap_or("")
                        .to_string();
                    turn_cwd = p
                        .get("cwd")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                }
            }
            "event_msg" if token_info.is_none() => {
                if let Some(p) = payload {
                    if p.get("type").and_then(|t| t.as_str()) == Some("token_count") {
                        token_info = p.get("info").cloned();
                    }
                }
            }
            _ => {}
        }
        if !model_id.is_empty() && token_info.is_some() {
            break;
        }
    }

    let context_window = token_info.as_ref().map(|info| {
        let last = info.get("last_token_usage");
        let get = |v: Option<&Value>, key: &str| -> u32 {
            v.and_then(|u| u.get(key))
                .and_then(|n| n.as_u64())
                .unwrap_or(0) as u32
        };
        let input = get(last, "input_tokens");
        let cached = get(last, "cached_input_tokens");
        let cache_write = get(last, "cache_write_input_tokens");
        let output = get(last, "output_tokens");
        ContextWindowInput {
            context_window_size: info
                .get("model_context_window")
                .and_then(|n| n.as_u64())
                .map(|n| n as u32),
            used_percentage: None,
            used_tokens: None,
            total_input_tokens: None,
            total_output_tokens: None,
            current_usage: Some(CurrentUsage {
                // codex input_tokens includes cached tokens; split them out
                // so the breakdown does not double-count
                input_tokens: Some(input.saturating_sub(cached)),
                output_tokens: Some(output),
                cache_creation_input_tokens: Some(cache_write),
                cache_read_input_tokens: Some(cached),
            }),
        }
    });

    let current_dir = if !turn_cwd.is_empty() {
        turn_cwd
    } else {
        meta_cwd
    };

    Some(InputData {
        model: Model {
            display_name: prettify_model(&model_id),
            id: model_id,
        },
        workspace: Workspace { current_dir },
        transcript_path: path.to_string_lossy().into_owned(),
        cost: None,
        output_style: None,
        session_id,
        context_window,
        pi: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rollout_fixture() {
        let dir = std::env::temp_dir().join("ccline-codex-test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout-2026-07-22-test.jsonl");
        let fixture = concat!(
            "{\"timestamp\":\"2026-07-22T09:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"abc-123\",\"cwd\":\"/tmp/proj\",\"originator\":\"codex-tui\",\"cli_version\":\"0.145.0\"}}\n",
            "{\"timestamp\":\"2026-07-22T09:00:01.000Z\",\"type\":\"turn_context\",\"payload\":{\"cwd\":\"/tmp/proj\",\"model\":\"gpt-5.4\",\"approval_policy\":\"on-request\"}}\n",
            "{\"timestamp\":\"2026-07-22T09:49:01.291Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":42878,\"cached_input_tokens\":0,\"cache_write_input_tokens\":0,\"output_tokens\":950,\"reasoning_output_tokens\":656,\"total_tokens\":43828},\"last_token_usage\":{\"input_tokens\":42878,\"cached_input_tokens\":40000,\"cache_write_input_tokens\":0,\"output_tokens\":950,\"reasoning_output_tokens\":656,\"total_tokens\":43828},\"model_context_window\":258400}}}\n",
        );
        fs::write(&path, fixture).unwrap();

        let input = parse_rollout(&path).expect("rollout should parse");
        assert_eq!(input.model.id, "gpt-5.4");
        assert_eq!(input.model.display_name, "GPT-5.4");
        assert_eq!(input.workspace.current_dir, "/tmp/proj");
        assert_eq!(input.session_id.as_deref(), Some("abc-123"));
        let cw = input.context_window.expect("context_window synthesized");
        assert_eq!(cw.context_window_size, Some(258400));
        let usage = cw.current_usage.expect("current usage");
        assert_eq!(usage.input_tokens, Some(2878)); // 42878 - 40000 cached
        assert_eq!(usage.cache_read_input_tokens, Some(40000));
        assert_eq!(usage.context_tokens(), 2878 + 40000 + 950);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn prettifies_codex_models() {
        assert_eq!(prettify_model("gpt-5.4"), "GPT-5.4");
        assert_eq!(prettify_model("gpt-5.5-codex"), "GPT-5.5 Codex");
        assert_eq!(prettify_model("o4-mini"), "o4-mini");
    }
}
