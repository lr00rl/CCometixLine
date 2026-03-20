# CCometixLine 全流程数据流详解

## 阶段 0: Claude Code 调用 ccline

Claude Code 在每次状态栏刷新时，通过 shell 调用 ccline 二进制，将 JSON 数据通过 **stdin** 管道传入：

```
echo '{"session_id":"...","transcript_path":"...","model":{...},...}' | ccometixline --logto ~/.claude/ccline/ccline.log --loglevel debug
```

---

## 阶段 1: `main.rs` — 入口与解析

```
main()
  ├── Cli::parse_args()          // clap 解析 CLI 参数
  ├── FileLogger::init()         // 如果有 --logto，初始化文件日志
  ├── stdin.read_to_string()     // 读取 stdin 全部内容 → raw
  ├── serde_json::from_str()     // raw → InputData 结构体
  ├── collect_all_segments()     // 收集所有 segment 数据
  ├── StatusLineGenerator::generate()  // 渲染为 ANSI 字符串
  └── println!("{}", statusline) // 输出到 stdout → Claude Code 读取
```

---

## 阶段 2: `InputData` — 两个数据源

ccline 有 **两个完全不同的数据源**：

### 数据源 A: stdin JSON（即时元数据）

```json
{
  "session_id": "98e79635-...",
  "transcript_path": "/Users/cdcd/.claude/projects/.../98e79635-....jsonl",
  "cwd": "/Users/cdcd/.../CCometixLine",
  "model": {"id": "claude-sonnet-4-6", "display_name": "Sonnet 4.6"},
  "workspace": {"current_dir": "/Users/cdcd/.../CCometixLine"},
  "version": "2.1.63",
  "output_style": {"name": "Structural Thinking"},
  "cost": {
    "total_cost_usd": 31.19,
    "total_duration_ms": 48666072,
    "total_lines_added": 1661,
    "total_lines_removed": 154
  },
  "context_window": {
    "current_usage": {
      "input_tokens": 1,
      "output_tokens": 361,
      "cache_creation_input_tokens": 72838,
      "cache_read_input_tokens": 0
    },
    "used_percentage": 36,
    "context_window_size": 200000
  }
}
```

反序列化为 `InputData` 结构体 (`types.rs:123-129`)：
- `input.model` → Model `{id, display_name}`
- `input.workspace` → Workspace `{current_dir}`
- `input.transcript_path` → 指向 JSONL 文件的路径
- `input.cost` → Cost `{total_cost_usd, total_duration_ms, ...}`
- `input.output_style` → OutputStyle `{name}`

### 数据源 B: transcript JSONL 文件（完整对话历史）

路径来自 `input.transcript_path`，文件内容是一行一个 JSON 对象的 JSONL 格式，包含整个会话的所有消息：

```jsonl
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"..."}]}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01X","name":"Edit","input":{"file_path":"config.toml",...}},{"type":"tool_use","id":"toolu_01Y","name":"Bash","input":{"command":"cargo build"}}],"usage":{"input_tokens":1000,"output_tokens":500,...}}}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_result","tool_use_id":"toolu_01X","content":"OK"}]}}
{"type":"progress","data":{"hookEvent":"PostToolUse","hookName":"Stop","command":"/path/to/run-with-flags.js"},"timestamp":"2026-03-20T11:19:00.000Z"}
{"type":"custom-title","title":"My Session Name"}
```

每行反序列化为 `TranscriptEntry` (`types.rs:438-451`)：
- `type` → `"user"` / `"assistant"` / `"progress"` / `"summary"` / `"custom-title"`
- `message.content[]` → 数组，每个元素是 `ContentBlock`
- `ContentBlock.type` → `"tool_use"` / `"tool_result"` / `"text"`
- `ContentBlock.id` → tool_use 的唯一 ID
- `ContentBlock.tool_use_id` → tool_result 引用的 tool_use ID
- `ContentBlock.name` → 工具名 (`"Edit"`, `"Bash"`, `"Skill"`, `"TaskCreate"`, ...)
- `ContentBlock.input` → 工具输入参数 (JSON Value)
- `message.usage` → token 使用量

---

## 阶段 3: `collect_all_segments()` — 逐段收集

`statusline.rs:380-483` 遍历 `config.segments` 列表，跳过 `enabled=false` 的段，对每个启用的 segment 调用对应的 `Segment::collect(input)` 方法。

每个 segment 返回 `Option<SegmentData>`：
```rust
pub struct SegmentData {
    pub primary: String,    // 主要显示文本
    pub secondary: String,  // 次要显示文本（较暗）
    pub metadata: HashMap<String, String>,  // 元数据（dynamic_icon 等）
}
```

下面是每个 segment 的数据流：

---

### 1. **Model** — 纯 stdin 数据
```
input.model.id = "claude-sonnet-4-6"
input.model.display_name = "Sonnet 4.6"
    │
    ▼
ModelConfig::load()  // 加载 models.toml 查找自定义显示名
    │
    ▼
primary = "Sonnet 4.6"
secondary = ""
```

### 2. **Directory** — 纯 stdin 数据
```
input.workspace.current_dir = "/Users/cdcd/.../CCometixLine"
    │
    ▼
extract_directory_name()  // 取路径最后一段
    │
    ▼
primary = "CCometixLine"
secondary = ""
```

### 3. **Git** — 外部命令（不读 stdin 也不读 JSONL）
```
input.workspace.current_dir → 工作目录
    │
    ├── git rev-parse --git-dir          // 检查是否是 git 仓库
    ├── git branch --show-current        // 获取分支名
    ├── git status --porcelain           // 获取文件状态 + 统计
    ├── git rev-list --count @{u}..HEAD  // ahead 计数
    ├── git rev-list --count HEAD..@{u}  // behind 计数
    └── (可选) git rev-parse --short=7 HEAD  // SHA
    │
    ▼
primary = "feat/add_session_detial"
secondary = "● !1 ?1"   // ●=dirty  !1=1个modified  ?1=1个untracked
```
`show_file_stats` 选项从 `segment_config.options["show_file_stats"]` 读取。

### 4. **Session** — stdin + JSONL + 文件缓存
```
input.cost.total_duration_ms = 48666072
input.cost.total_lines_added = 1661
input.cost.total_lines_removed = 154
    │
    ├── format_duration(48666072) → "13h31m"
    │
    ├── 行变化: "\x1b[32m+1661\x1b[0m \x1b[31m-154\x1b[0m"
    │
    └── calculate_speed():
        ├── 读取 JSONL: 遍历所有 type="assistant" 条目
        │   └── 累加每个 message.usage.output_tokens → current_tokens
        ├── 读取 ~/.claude/ccline/.speed_cache.json
        │   └── {output_tokens: 上次值, timestamp_ms: 上次时间}
        ├── speed = (current_tokens - 上次) / (now - 上次时间)
        └── 写入新的 speed_cache
    │
    ▼
primary = "13h31m"
secondary = "+1661 -154 · 15.2 tok/s"
```

### 5. **OutputStyle** — 纯 stdin 数据
```
input.output_style.name = "Structural Thinking"
    │
    ▼
primary = "Structural Thinking"
secondary = ""
```

### 6. **ContextWindow** — JSONL（从后往前扫描）
```
input.transcript_path → 打开 JSONL 文件
input.model.id → 查询 context limit
    │
    ├── ModelConfig::get_context_limit("claude-sonnet-4-6") → 200000
    │
    ├── parse_transcript_usage():
    │   └── 从 JSONL 末尾往前找第一个 type="assistant" 条目
    │       └── message.usage → RawUsage.normalize() → NormalizedUsage
    │           └── display_tokens() = input + cache_creation + cache_read + output
    │                                = 1 + 72838 + 0 + 361 = 73200
    │
    ├── rate = 73200 / 200000 * 100 = 36.6%
    │
    ├── progress_bar(36.6, 10) → "████░░░░░░"
    │
    └── (如果 rate >= 85%) parse_token_breakdown() → 显示 "in: Xk, cache: Yk"
    │
    ▼
primary = "████░░░░░░ 36.6%"
secondary = ""   // (因为 36.6% < 85%)
```

**注意**: 如果 JSONL 末尾是 `type="summary"`（context compaction 后），会通过 `leafUuid` 在项目目录的其他 JSONL 文件中查找对应的 assistant 消息。

### 7. **Usage** — API 调用 + 文件缓存
```
credentials::get_oauth_token() → OAuth token
    │
    ├── 检查缓存: ~/.claude/ccline/.api_usage_cache.json
    │   ├── is_cache_valid(): 检查 cached_at + cache_duration(300s)
    │   └── 如果有效 → 直接用缓存值 ("usage: using cached data")
    │
    ├── 如果无效 → fetch_api_usage():
    │   ├── GET https://api.anthropic.com/api/oauth/usage
    │   │   Headers: Authorization: Bearer {token}
    │   │            anthropic-beta: oauth-2025-04-20
    │   ├── 响应: {five_hour: {utilization: 0.0}, seven_day: {utilization: 63.0, resets_at: "..."}}
    │   └── 写入缓存
    │
    ├── progress_bar(0.0, 10) → "░░░░░░░░░░"
    ├── progress_bar(63.0, 10) → "██████░░░░"
    ├── format_5h_elapsed(0.0) → "0m"
    └── format_7d_elapsed(63.0) → "4d 10h"
    │
    ▼
primary = "░░░░░░░░░░ 0% (0m / 5h) | ██████░░░░ 63% (4d 10h / 7d)"
secondary = ""
metadata: dynamic_icon = circle_slice_6 (根据 7d utilization)
```

### 8. **Cost** — 纯 stdin 数据
```
input.cost.total_cost_usd = 31.195686
    │
    ▼
primary = "$31.20"
secondary = ""
```

### 9. **Tools** — JSONL（全文扫描）
```
input.transcript_path → 打开 JSONL，逐行扫描
    │
    ├── 遇到 type="tool_use" 的 ContentBlock:
    │   ├── 跳过 name="Skill"（留给 Skills segment）
    │   ├── 记录 {id, name, completed=false, arg}
    │   └── extract_key_arg() 提取关键参数:
    │       ├── Edit/Read/Write → file_path → shorten_path() 取文件名
    │       │   例: "/Users/.../config.toml" → "config.toml"
    │       ├── Bash → command (截断35字符)
    │       │   例: "cargo build --release 2>&1"
    │       ├── Grep/Glob → pattern
    │       ├── Agent/Task → subagent_type
    │       └── WebFetch → URL domain
    │
    ├── 遇到 type="tool_result" 的 ContentBlock:
    │   └── 通过 tool_use_id 找到对应记录，标记 completed=true
    │
    ├── HashMap → Vec，按 id 排序（近似插入顺序），保留最后100条
    │
    ├── show_args=true 时（Detail 模式）:
    │   └── 最多2个 running + 3个最近 completed，带参数
    │       "✓ Edit config.toml | ✓ Bash cargo build --release 2>&1 | ✓ TskUpd"
    │
    └── show_args=false 时（Compact 模式）:
        └── running + 前4个 completed 按频率排序
            "✓ Read ×22 ✓ Bash ×18 ✓ TskUpd ×15 ✓ Edit ×12"
    │
    ▼
primary = "✓ Edit config.toml | ✓ Bash cargo build ... | ✓ TskUpd"
secondary = "=> Read×22 Bash×18 TskUpd×15 Edit×12 Agent×11 ..."
```

**这就是你问的 "Edit config.toml" 的来源** — 它来自 JSONL 文件中的一条 `tool_use` 记录，不在 stdin raw 里。

### 10. **Todos** — JSONL（全文扫描）
```
input.transcript_path → 打开 JSONL，逐行扫描
    │
    ├── 遇到 name="TodoWrite":
    │   └── input.todos[] → 替换整个列表（老的 TodoWrite 方式）
    │       每个 todo: {id, content, status, priority}
    │
    ├── 遇到 name="TaskCreate":
    │   └── task_seq++ → id = "1","2","3"...
    │       content = input.subject
    │       status = "pending"
    │
    ├── 遇到 name="TaskUpdate":
    │   ├── status="deleted" → 从列表中移除（retain）
    │   └── 其他 status → 更新匹配 taskId 的 todo
    │
    ├── 最终得到 latest_todos 列表
    │
    ├── 统计: total, completed_count, in_progress
    │
    ├── 有 in_progress 任务时:
    │   └── "✓ 前一个 -→ ▶ 当前任务 (5/11) -→ › 下一个"
    ├── 全部 completed 时:
    │   └── "✓ 倒数第2 → ✓ 最后一个 ✦ 11/11"
    └── 有 pending 无 in_progress 时:
        └── "5/11 › 第一个pending的内容"
    │
    ▼
primary = "5/11 › Task 2: Add Segme…"  // (修复前，deleted 任务膨胀了 total)
```

### 11. **Environment** — 文件系统扫描（不读 JSONL）
```
input.workspace.current_dir → cwd
    │
    ├── get_claude_md_paths(): 扫描 ~/.claude/CLAUDE.md 和 cwd/.claude/CLAUDE.md
    │
    ├── get_rule_names(): 扫描 ~/.claude/rules/ 和 cwd/.claude/rules/ 中的 .md 文件
    │   └── 取文件名（不含扩展名）→ ["agents", "coding-style", "development-workflow", ...]
    │
    ├── get_mcp_and_hook_names(): 解析 settings.json
    │   ├── settings.mcpServers → MCP 名称列表
    │   └── settings.hooks → hook 事件名 + 命令
    │
    ├── show_names=true:
    │   └── "rules: agents, coding-style, ... | MCPs: mira-api-dev-http, ..."
    └── show_names=false:
        └── "9 rules · 3 MCPs · 4 hooks"
    │
    ▼
primary = "rules: agents, coding-style, development-workflow, ..."
secondary = ""
```

### 12. **SessionName** — JSONL（全文扫描）
```
input.transcript_path → 逐行找 type="custom-title"
    │
    └── 取最后一个 title 字段值
    │
    ▼
primary = "My Session Name"  // 或 None（无自定义标题时不显示）
```

### 13. **Skills** — JSONL（全文扫描）
```
input.transcript_path → 逐行找 name="Skill" 的 tool_use
    │
    ├── tool_use: 记录 {id, skill_name=input.skill, completed=false}
    ├── tool_result: 通过 tool_use_id 匹配，标记 completed=true
    │
    ├── skill_name 去命名空间前缀:
    │   "superpowers:brainstorming" → "brainstorming"
    │
    ├── 最多3个 running + 6个 completed
    │   "✓ subagent-driven-development | ✓ writing-plans | ✓ brainstorming | ✓ finishing-a-development-branch"
    │
    ▼
primary = "✓ subagent-driven-... | ✓ writing-plans | ✓ brainstorming | ..."
secondary = "=> 4 skills"
```

**注意**: Skills 和 Tools 互斥 — Tools 跳过 `name="Skill"` 的条目，Skills 只处理 `name="Skill"` 的条目。

一条 Skill 的记录是这样的：

```json
// jq -c 'select(any(.message.content[]?; .name == "Skill"))' *.jsonl
{
  "parentUuid": "a0a0c525-2ce5-401b-9144-4ade76144c3e",
  "isSidechain": false,
  "userType": "external",
  "cwd": "/Users/cdcd/roobli/RTFS_justTaste/custom_claude_statusline",
  "sessionId": "98e79635-989e-4bd2-b756-861bdb810341",
  "version": "2.1.63",
  "gitBranch": "HEAD",
  "message": {
    "id": "gen-1773925271-shBLZ8RZSk310Anrrjb9",
    "type": "message",
    "role": "assistant",
    "container": null,
    "content": [
      {
        "type": "tool_use",
        "id": "toolu_vrtx_01KabLmVRd2n6kxoH22chFdR",
        "caller": {
          "type": "direct"
        },
        "name": "Skill",
        "input": {
          "skill": "superpowers:brainstorming"
        }
      }
    ],
    "model": "anthropic/claude-4.6-sonnet-20260217",
    "stop_reason": null,
    "stop_sequence": null,
    "usage": {
      "input_tokens": 0,
      "output_tokens": 0,
      "cache_creation_input_tokens": null,
      "cache_read_input_tokens": null,
      "cache_creation": null,
      "inference_geo": null,
      "server_tool_use": null,
      "service_tier": null
    },
    "provider": "Google"
  },
  "type": "assistant",
  "uuid": "5624853a-1ed0-4a53-85ec-460950ce55e0",
  "timestamp": "2026-03-19T13:01:22.081Z"
}
```

### 14. **Hooks** — JSONL（全文扫描，匹配 progress 事件）
```
input.transcript_path → 逐行找 type="progress" 且含 data.hook_event
    │
    ├── 每条: {hook_event, hook_name, script_name, timestamp_secs}
    │   ├── hook_event: "PostToolUse", "PreToolUse", "Stop", "SessionStart"
    │   ├── hook_name: 具体钩子名称
    │   ├── script_name: 从 data.command 提取脚本文件名
    │   └── timestamp_secs: 解析 ISO 8601 → unix 秒
    │
    ├── is_running(): now - timestamp < 5秒 → 认为正在运行
    │
    ├── 最多2个 running + 4个 completed
    │   show_args=true: "◐ Stop run-with-flags.js | ✓ PostToolUse:Bash run-with-flags.js"
    │
    └── secondary: 按事件类型统计
        "=> Post×20  Pre×16  Stop×10  Start×4"
    │
    ▼
primary = "◐ Stop run-with-flags.js | ✓ PostToolUse:Bash run-with-flags.js"
secondary = "=> Post×20  Pre×16  Stop×10  Start×4"
```

### 15. **Agents** — JSONL（全文扫描，匹配 Task 工具）
```
input.transcript_path → 逐行找 name="Task" 的 tool_use
    │
    ├── tool_use: {subagent_type, description, model, start_time=行号}
    ├── tool_result: 标记 completed=true, duration_s=行号差
    │
    ├── 最多2个 running + 1个 completed
    │   "◐ code-reviewer [sonnet] · Review auth module"
    │
    ▼
primary = "◐ code-reviewer [sonnet] · Review auth..."
secondary = ""
```

---

## 阶段 4: `StatusLineGenerator::generate()` — 渲染

```
collect_all_segments() 返回 Vec<(SegmentConfig, SegmentData)>
    │
    ├── 过滤掉 enabled=false 的段
    │
    ├── 按 segment_config.line 分组 (BTreeMap<u8, Vec<...>>)
    │   └── line=0 的放一行，line=1 的放下一行...
    │
    ├── 对每行内的每个 segment:
    │   render_segment(config, data):
    │     │
    │     ├── 选择 icon:
    │     │   ├── metadata 中有 "dynamic_icon" → 用它（如 Usage 的圆饼图标）
    │     │   └── 否则按 style.mode 选: Plain → icon.plain, NerdFont → icon.nerd_font
    │     │
    │     ├── 应用颜色:
    │     │   ├── icon: config.colors.icon → \x1b[38;2;R;G;Bm...\x1b[0m
    │     │   ├── text: config.colors.text + config.styles.text_bold → \x1b[1;38;5;Cm...\x1b[0m
    │     │   └── background: config.colors.background → \x1b[48;2;R;G;Bm...\x1b[49m
    │     │
    │     └── 拼接: " {icon} {primary} {secondary} "
    │
    ├── 拼接同一行的 segments:
    │   ├── Powerline 模式 (separator=""):
    │   │   └── join_with_powerline_arrows()
    │   │       前一段的 bg → 箭头 fg，下一段的 bg → 箭头 bg
    │   │       \x1b[bg2]\x1b[fg1]\x1b[0m
    │   │
    │   └── 普通模式:
    │       └── join_with_white_separators()
    │           \x1b[37m{separator}\x1b[0m
    │
    └── 多行之间用 \n 连接
    │
    ▼
最终输出: println!("{}", statusline) → stdout → Claude Code 读取并显示
```

---

## 总结: 数据源分类

| Segment | stdin JSON | JSONL 文件 | 外部命令 | 文件系统 | API |
|---------|:---------:|:----------:|:-------:|:-------:|:---:|
| Model | `model.id/display_name` | | | models.toml | |
| Directory | `workspace.current_dir` | | | | |
| Git | `workspace.current_dir` | | `git` x4-5 | | |
| Session | `cost.*` | `usage` (tok/s) | | speed_cache.json | |
| OutputStyle | `output_style.name` | | | | |
| ContextWindow | `model.id` | `usage` (最后assistant) | | | |
| Usage | | | | api_usage_cache.json | Anthropic API |
| Cost | `cost.total_cost_usd` | | | | |
| Tools | | `tool_use/result` | | | |
| Todos | | `TaskCreate/Update` | | | |
| Environment | `workspace.current_dir` | | | rules/settings | |
| SessionName | | `custom-title` | | | |
| Skills | | `Skill` tool_use | | | |
| Hooks | | `progress` events | | | |
| Agents | | `Task` tool_use | | | |
