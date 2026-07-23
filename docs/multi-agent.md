# Multi-Agent Support

CCometixLine speaks the Claude Code statusline protocol (JSON on stdin) and
extends it to other AI coding agents. Status as of 2026-07:

| Agent | Mechanism | Status |
|-------|-----------|--------|
| Claude Code | native `statusLine` command | ✅ full support |
| pi | native `statusLine` command (pi-statusline) | ✅ full support, honors `context_window` |
| Kimi (K3 / K2.7) | via Claude Code + Moonshot Anthropic endpoint | ✅ works, models recognized |
| Codex CLI | `ccline --codex` reads rollout sessions | ✅ sidecar / tmux mode |

## Input dialects

`InputData` parsing is lenient: every field is optional, so any agent that
sends a subset of the Claude Code payload renders whatever segments have data.

Beyond the Claude Code baseline (`model`, `workspace`, `transcript_path`,
`cost`, `output_style`), ccline understands a host-provided context block and
prefers it over transcript parsing + `models.toml` limits:

```jsonc
// pi dialect ("context_window")
{
  "context_window": {
    "context_window_size": 1048576,
    "used_percentage": 36,
    "current_usage": {
      "input_tokens": 841,
      "output_tokens": 956,
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 96768
    }
  }
}

// kimi-code statusline proposal dialect ("context", PR MoonshotAI/kimi-code#2043)
{
  "context": { "used_tokens": 50000, "max_tokens": 1048576, "percent": 4.8 }
}
```

Priority for the ContextWindow segment:

1. `context_window.context_window_size` → context limit (else `models.toml` / built-ins)
2. `context_window.current_usage` sum → used tokens (else `used_tokens`, else
   `total_input_tokens + total_output_tokens`, else transcript parsing)

## Claude Code

Native. Add to `~/.claude/settings.json`:

```json
{
  "statusLine": { "type": "command", "command": "~/.claude/ccline/ccline", "padding": 0 }
}
```

## pi

pi supports the same protocol via its `pi-statusline` package. In
`~/.pi/agent/settings.json`:

```json
{
  "packages": ["npm:pi-statusline"],
  "statusLine": {
    "type": "command",
    "command": "~/.claude/ccline/ccline"
  }
}
```

pi writes a minimal Claude-compatible transcript to
`~/.pi/agent/statusline-transcripts/<session>.jsonl` (usage only, no content
blocks), so transcript-based segments (Tools / Skills / Todos / Agents) stay
empty under pi. Context, model, git, directory, cost all work. The payload's
`pi.session_file` points at pi's full native session log for future use.

## Kimi

The Kimi Code CLI (MoonshotAI/kimi-code) has **no statusline hook yet**
(tracked in issue #1954; community PRs #2043 / #1493 unmerged). The supported
path is running Kimi models **through Claude Code** via Moonshot's
Anthropic-compatible endpoint:

```bash
export ANTHROPIC_BASE_URL="https://api.moonshot.ai/anthropic"
export ANTHROPIC_AUTH_TOKEN="$MOONSHOT_API_KEY"
export ANTHROPIC_MODEL="kimi-k3"          # or kimi-k2.7-code
# subscription variant: ANTHROPIC_BASE_URL=https://api.kimi.com/coding/  ANTHROPIC_MODEL="k3[1m]"
```

ccline then works unchanged. Built-in model recognition:

| Pattern | Display | Context limit |
|---------|---------|---------------|
| `k3` (matches `kimi-k3`, `k3[1m]`) | Kimi K3 | 1,048,576 |
| `kimi-k2.7-code` (incl. `-highspeed`) | Kimi K2.7 Code | 262,144 |
| `kimi-k2.6` | Kimi K2.6 | 262,144 |
| `kimi-k2-turbo` / `kimi-k2` (legacy) | Kimi K2 (Turbo) | 262,144 |

Override or extend in `~/.claude/ccline/models.toml`.

## Codex CLI

Codex's `[tui] status_line` only accepts built-in item ids — there is no
external statusline command hook (openai/codex#17827 still open), and unknown
items are silently dropped. ccline therefore ships a pull mode that reads
Codex rollout session files directly:

```bash
ccline --codex                       # auto-detect the active session
ccline --codex-session ~/.codex/sessions/2026/07/22/rollout-....jsonl
```

Auto-detection scans `$CODEX_HOME/sessions` (default `~/.codex/sessions`) for
the newest `rollout-*.jsonl`, skipping subagent threads (whose rollouts replay
parent token history), and prefers a session whose recorded `cwd` matches the
current directory — so per-project panes show their own session.

Parsed from the rollout: model (`turn_context`), cwd, session id, and the last
`token_count` event (`last_token_usage` + `model_context_window`) which feeds
the ContextWindow segment with a cache-aware breakdown.

Usage examples:

```bash
# tmux status bar (refreshes with tmux status-interval)
set -g status-interval 5
set -g status-right "#(cd #{pane_current_path} && ~/.claude/ccline/ccline --codex)"

# watch in a sidecar pane
watch -n 5 '~/.claude/ccline/ccline --codex'
```

Note: tmux strips ANSI colors from `#()` output unless you use a plain theme
or pipe through `tmux`-compatible formatting; the sidecar `watch --color` mode
preserves colors.
