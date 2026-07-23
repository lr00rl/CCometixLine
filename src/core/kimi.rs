//! Kimi Code CLI adapter.
//!
//! kimi-code (MoonshotAI/kimi-code) has no external statusline hook yet
//! (tracked in issue #1954), so like the codex adapter we synthesize an
//! `InputData` by reading its session files under `$KIMI_CODE_HOME`
//! (default `~/.kimi-code`). Intended for tmux popups / sidecar panes:
//!
//! ```text
//! ccline --kimi                 # auto-detect newest session (prefers cwd match)
//! ccline --kimi-session ~/.kimi-code/sessions/<wd>/<session>   # or its wire.jsonl
//! ```
//!
//! Layout:
//!   - `session_index.jsonl`: `{"sessionId","sessionDir","workDir"}` per line
//!   - `<sessionDir>/agents/main/wire.jsonl`: op stream; we use
//!     `{"type":"usage.record","model":"kimi-code/k3","usage":{"inputOther",
//!     "output","inputCacheRead","inputCacheCreation"},...}` (last one wins)
//!     and `{"type":"config.update","modelAlias":...}` as model fallback
//!   - `config.toml`: `[models."kimi-code/k3"] max_context_size, display_name`

use crate::config::{ContextWindowInput, CurrentUsage, InputData, Model, Workspace};
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn build_input(explicit_session: Option<&str>) -> Option<InputData> {
    let (wire_path, work_dir) = match explicit_session {
        Some(p) => resolve_explicit(p)?,
        None => find_active_session()?,
    };
    crate::log_debug!("kimi: using wire {:?} workDir={:?}", wire_path, work_dir);
    parse_session(&wire_path, work_dir)
}

fn kimi_home() -> PathBuf {
    std::env::var_os("KIMI_CODE_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".kimi-code")))
        .unwrap_or_else(|| PathBuf::from(".kimi-code"))
}

/// Accept either a session directory or a direct wire.jsonl path.
fn resolve_explicit(p: &str) -> Option<(PathBuf, Option<String>)> {
    let path = PathBuf::from(p);
    let wire = if path.is_dir() {
        path.join("agents").join("main").join("wire.jsonl")
    } else {
        path
    };
    if wire.exists() {
        Some((wire, None))
    } else {
        None
    }
}

/// Pick the newest session (by wire.jsonl mtime) from session_index.jsonl,
/// preferring one whose workDir matches the current working directory.
fn find_active_session() -> Option<(PathBuf, Option<String>)> {
    let index_path = kimi_home().join("session_index.jsonl");
    let content = fs::read_to_string(&index_path).ok()?;
    let current_dir = std::env::current_dir()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();

    // (mtime, wire_path, work_dir)
    let mut sessions: Vec<(std::time::SystemTime, PathBuf, String)> = Vec::new();
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let Some(session_dir) = v.get("sessionDir").and_then(|s| s.as_str()) else {
            continue;
        };
        let work_dir = v
            .get("workDir")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let wire = Path::new(session_dir)
            .join("agents")
            .join("main")
            .join("wire.jsonl");
        let Ok(meta) = fs::metadata(&wire) else {
            continue; // stale index entry
        };
        let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        sessions.push((mtime, wire, work_dir));
    }

    sessions.sort_by(|a, b| b.0.cmp(&a.0));

    let cwd_match = sessions
        .iter()
        .find(|(_, _, wd)| !current_dir.is_empty() && *wd == current_dir);
    let chosen = cwd_match.or_else(|| sessions.first())?;
    Some((chosen.1.clone(), Some(chosen.2.clone())))
}

/// Look up `max_context_size` and `display_name` for a model alias from
/// kimi-code's own config.toml — authoritative and self-maintaining.
fn model_info_from_config(alias: &str) -> (Option<u32>, Option<String>) {
    let config_path = kimi_home().join("config.toml");
    let Ok(content) = fs::read_to_string(&config_path) else {
        return (None, None);
    };
    let Ok(value) = content.parse::<toml::Value>() else {
        return (None, None);
    };
    let model = value.get("models").and_then(|m| m.get(alias));
    let limit = model
        .and_then(|m| m.get("max_context_size"))
        .and_then(|v| v.as_integer())
        .map(|v| v as u32);
    let display = model
        .and_then(|m| m.get("display_name"))
        .and_then(|v| v.as_str())
        .map(String::from);
    (limit, display)
}

fn parse_session(wire_path: &Path, work_dir: Option<String>) -> Option<InputData> {
    let mut raw = String::new();
    fs::File::open(wire_path)
        .ok()?
        .read_to_string(&mut raw)
        .ok()?;

    // Scan backwards for the latest usage.record (usage + model) and, as a
    // model fallback, the latest config.update carrying modelAlias.
    let mut model_alias = String::new();
    let mut usage: Option<CurrentUsage> = None;
    for line in raw.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "usage.record" if usage.is_none() => {
                if let Some(u) = v.get("usage") {
                    let get = |key: &str| -> Option<u32> {
                        u.get(key).and_then(|n| n.as_u64()).map(|n| n as u32)
                    };
                    usage = Some(CurrentUsage {
                        input_tokens: get("inputOther"),
                        output_tokens: get("output"),
                        cache_creation_input_tokens: get("inputCacheCreation"),
                        cache_read_input_tokens: get("inputCacheRead"),
                    });
                }
                if model_alias.is_empty() {
                    if let Some(m) = v.get("model").and_then(|m| m.as_str()) {
                        model_alias = m.to_string();
                    }
                }
            }
            "config.update" if model_alias.is_empty() => {
                if let Some(m) = v.get("modelAlias").and_then(|m| m.as_str()) {
                    model_alias = m.to_string();
                }
            }
            _ => {}
        }
        if usage.is_some() && !model_alias.is_empty() {
            break;
        }
    }

    let (limit, display_name) = model_info_from_config(&model_alias);
    // Short id without the provider prefix, e.g. "kimi-code/k3" -> "k3"
    let model_id = model_alias
        .rsplit('/')
        .next()
        .unwrap_or(&model_alias)
        .to_string();

    // Resolve workDir from the sibling state.json when the index didn't tell us
    let current_dir = work_dir
        .or_else(|| {
            let state = wire_path.parent()?.parent()?.parent()?.join("state.json");
            let content = fs::read_to_string(state).ok()?;
            let v: Value = serde_json::from_str(&content).ok()?;
            v.get("workDir")
                .and_then(|w| w.as_str())
                .map(String::from)
        })
        .unwrap_or_default();

    let context_window = if limit.is_some() || usage.is_some() {
        Some(ContextWindowInput {
            context_window_size: limit,
            used_percentage: None,
            used_tokens: None,
            total_input_tokens: None,
            total_output_tokens: None,
            current_usage: usage,
        })
    } else {
        None
    };

    Some(InputData {
        model: Model {
            display_name: display_name.unwrap_or_else(|| model_id.clone()),
            id: model_id,
        },
        workspace: Workspace { current_dir },
        transcript_path: wire_path.to_string_lossy().into_owned(),
        cost: None,
        output_style: None,
        session_id: None,
        context_window,
        pi: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wire_fixture() {
        let dir = std::env::temp_dir().join("ccline-kimi-test/agents/main");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("wire.jsonl");
        let fixture = concat!(
            "{\"type\":\"metadata\",\"protocol_version\":\"1.4\",\"created_at\":1784262531433}\n",
            "{\"type\":\"config.update\",\"modelAlias\":\"kimi-code/k3\",\"thinkingEffort\":\"max\",\"time\":1784262650254}\n",
            "{\"type\":\"usage.record\",\"model\":\"kimi-code/k3\",\"usage\":{\"inputOther\":11299,\"output\":512,\"inputCacheRead\":19200,\"inputCacheCreation\":0},\"usageScope\":\"turn\",\"time\":1784263123079}\n",
        );
        fs::write(&path, fixture).unwrap();

        let input = parse_session(&path, Some("/tmp/proj".into())).expect("wire should parse");
        assert_eq!(input.model.id, "k3");
        assert_eq!(input.workspace.current_dir, "/tmp/proj");
        let cw = input.context_window.expect("context_window synthesized");
        let usage = cw.current_usage.expect("usage present");
        assert_eq!(usage.input_tokens, Some(11299));
        assert_eq!(usage.cache_read_input_tokens, Some(19200));
        assert_eq!(usage.context_tokens(), 11299 + 512 + 19200);

        fs::remove_file(&path).ok();
    }
}
