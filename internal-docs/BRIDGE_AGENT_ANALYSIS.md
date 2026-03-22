# Bridge Agent 逻辑分析

> 目标：梳理 Bridge 中 6 个 Agent 的全部逻辑，为未来配置驱动架构重构提供设计依据。
> 创建日期：2026-03-23 | 当前版本：v3.2.6

---

## 1. 当前架构概述

Bridge（`bridge-src/src/main.rs`，约 1506 行）运行在远程服务器上（或本地），功能：
1. 从 stdin 读取 JSON 命令（通过 SSH 或直接管道）
2. 调用 Agent CLI
3. 解析输出 → 返回 JSON 响应

**已部分配置化**：`load_config()` 支持 `plugin.json` 和 `--config` CLI 参数来构建命令（`BridgeConfig`）。但另外 5 个子系统仍然完全硬编码。

---

## 2. 六大子系统中的 Agent 特定代码

| # | 子系统 | 函数 | 行数 | Agent 特定？ |
|---|--------|------|------|-------------|
| 1 | 命令构建 | `BridgeConfig` + `execute_chat()` | 140-340 | ⚠️ 部分 — `plugin.json` 已有，但角色前置注入仍硬编码 |
| 2 | 输出解析 | `parse_agent_output()` | 404-556 | ❌ 完全硬编码（4 种 JSON 格式 + 纯文本模式） |
| 3 | 日志过滤 | `is_agent_log_line()` | 1243-1293 | ❌ 完全硬编码（7 种匹配模式） |
| 4 | Agent 检测 | `KNOWN_AGENTS` + `handle_detect_agents()` | 584-670 | ❌ 硬编码 Agent 列表 |
| 5 | 角色管理 | `handle_set_role()` + `handle_clear_role()` | 681-910 | ❌ 每个 Agent 完全不同 |
| 6 | 模型配置 | `handle_set_model()` | 915-1155 | ❌ 每个 Agent 完全不同 |

---

## 3. 逐 Agent 详细分析

### 3.1 OpenClaw（默认 Agent）

| 维度 | 详情 |
|------|------|
| **命令** | `openclaw agent --json --agent main --message {msg}` |
| **输出** | JSON：`result.payloads[].text` 或 `payloads[].text` |
| **会话** | `result.meta.agentMeta.sessionId` |
| **日志过滤** | Rust tracing（`INFO/WARN/ERROR`）、`key=value` 行 |
| **角色** | 共享文件：`~/.openclaw/workspace/SOUL.md`（始终覆盖） |
| **角色清除** | 截断 SOUL.md 为 0 字节 |
| **模型配置** | 写入 `~/.openclaw/openclaw.json` — 复杂结构：`providers.eb_{host}`、`agents.defaults.model.primary`、自动检测 `anthropic-messages` vs `openai-completions`、保留 `gateway` token |
| **特殊** | 🦞 分隔符提取、`--message` 标志 |

#### 边界情况与陷阱：
- Gateway token 必须从现有配置中保留（关键业务逻辑）
- Provider 标签从 base_url 主机名派生（如 `eb_openrouter`）
- API 类型自动检测：模型名含 "claude" 或 URL 含 "anthropic" 时为 `anthropic`

---

### 3.2 Claude Code

| 维度 | 详情 |
|------|------|
| **命令** | `claude -p --output-format=json {msg}`（通过 `plugin.json`） |
| **输出** | JSON：4 种格式 — `result` 字符串、`result.content[].text`、`result.text`、`is_error` 标志 |
| **会话** | `session_id`（顶层 JSON 字段） |
| **日志过滤** | 无特定（通用模式适用） |
| **角色** | 独立文件：`~/.claude/agents/{role_id}.md`（永久保留，已存在则不覆盖） |
| **角色清除** | 保留文件，仅停止传递 `--agent` 参数 |
| **模型配置** | 写入两个文件：`~/.claude.json`（跳过引导）+ `~/.claude/settings.json`（环境变量 + allowedTools） |
| **特殊** | `--agent {name}` 标志、YAML frontmatter `name` 字段提取 |

#### 边界情况与陷阱：
- ⚠️ **YAML frontmatter 解析**：Claude Code 按 YAML `name` 字段匹配（如 "叙事设计师"），不按文件名匹配。Bridge 必须从下载的 `.md` 文件中提取 `name`
- 引导跳过文件（`~/.claude.json`）仅在不存在时写入
- `settings.json` 包含 5 个模型覆盖环境变量（`ANTHROPIC_MODEL` 等）
- `allowedTools` 数组必须正确，否则 Claude Code 无法工作
- 输出 JSON 中的 `is_error` 标志

---

### 3.3 ZeroClaw

| 维度 | 详情 |
|------|------|
| **命令** | 通过 `--command` 或 `plugin.json` 自定义 |
| **输出** | 纯文本（无 JSON） |
| **会话** | 无 |
| **日志过滤** | Rust tracing 模式 |
| **角色** | 独立文件：`~/.zeroclaw/workspace/skills/{role_id}/SKILL.md` |
| **角色清除** | 保留文件，停止传递角色参数 |
| **模型配置** | 写入 `~/.zeroclaw/config.toml` — TOML 格式：`default_provider`、`default_model`、`api_key`。同时设置环境变量 `OPENROUTER_API_KEY` + `OPENAI_API_KEY` |
| **特殊** | 从 URL 检测 Provider（openrouter/anthropic/openai/custom） |

#### 边界情况与陷阱：
- TOML 格式（非 JSON）— 需要不同的文件写入逻辑
- 自定义 provider 使用 `custom:{base_url}` 格式
- 环境变量通过 `set_var` 设置 — 进程级副作用
- Skill 路径深层嵌套：`skills/{role_id}/SKILL.md`

---

### 3.4 NanoBot

| 维度 | 详情 |
|------|------|
| **命令** | 通过 `plugin.json` 自定义 |
| **输出** | 纯文本（无 JSON），可能含 `<think>` 标签 |
| **会话** | 无 |
| **日志过滤** | `🐈 nanobot` 横幅 |
| **角色** | 共享文件：`~/.nanobot/workspace/SOUL.md` |
| **角色清除** | 截断 SOUL.md 为 0 字节 |
| **模型配置** | 写入 `~/.nanobot/config.json` — 简单结构：`agents.defaults.model` + `providers.custom.apiBase/apiKey` |
| **特殊** | 角色消息前置（系统提示注入到消息文本中）、`ensure_v1_suffix()` 处理 base URL |

#### 边界情况与陷阱：
- 消息前置格式：`[Context: ...]\n{角色}\n\n[User Query]\n{消息}`
- `ensure_v1_suffix()` 在 base URL 缺少 `/v1` 时自动追加

---

### 3.5 PicoClaw

| 维度 | 详情 |
|------|------|
| **命令** | 通过 `plugin.json` 自定义 |
| **输出** | 含 ANSI 代码的纯文本、🦞 分隔符、`<think>` 标签 |
| **会话** | 无 |
| **日志过滤** | Go 风格日志（`14:22:15 INF ...`）、ASCII 横幅（`█╗║╔╚═╝`）、PicoClaw 文字横幅 |
| **角色** | 共享文件：`~/.picoclaw/workspace/SOUL.md` |
| **角色清除** | 截断 SOUL.md 为 0 字节 |
| **模型配置** | 写入 `~/.picoclaw/config.json` — `model_list` 数组格式：`model_name`、vendor 前缀（`openai/` 或 `anthropic/`）、`api_key`、`api_base` |
| **特殊** | ANSI 剥离必须在日志过滤之前执行、🦞 分隔符提取 |

#### 边界情况与陷阱：
- ANSI 颜色代码会破坏模式匹配，必须先剥离
- 🦞 分隔符：提取最后一个龙虾 emoji 之后的所有内容
- `<think>` 标签剥离（NanoBot、Hermes 也有）
- 模型 ID 的 vendor 前缀检测（`anthropic/` vs `openai/`）

---

### 3.6 Hermes Agent

| 维度 | 详情 |
|------|------|
| **命令** | `hermes chat -Q -q {msg}`（通过 `plugin.json`） |
| **输出** | 纯文本，可能含 `session_id:` 尾注 |
| **会话** | 从最后一行提取：`session_id: xxx` |
| **日志过滤** | `Hermes`/`hermes` 横幅、方框绘制边框（`╭╰└┌`） |
| **角色** | 共享文件：`~/.hermes/SOUL.md` |
| **角色清除** | 截断 SOUL.md 为 0 字节 |
| **模型配置** | 复杂多步骤：`hermes config set model.default`、`sed .env` 处理 API 密钥/URL、删除 `ANTHROPIC_API_KEY`、清理旧条目 |
| **特殊** | 角色消息前置、通过 `hermes config set` CLI + `.env` 文件操作配置 |

#### 边界情况与陷阱：
- ⚠️ **最复杂的模型配置**：同时使用 CLI 命令（`hermes config set`）和文件操作（`sed` 修改 `.env`）
- `ANTHROPIC_API_KEY` 必须删除，否则 Hermes 会默认使用 Anthropic
- `.env` 中的旧条目必须清理（`LLM_MODEL`、`OPENAI_MODEL`、`OPENAI_BASE_URL`）
- `config.yaml` 中的旧条目也必须清理
- 会话 ID 从纯文本输出解析（非 JSON）
- 角色注入使用消息前置方式（同 NanoBot）

---

## 4. 模式分类

| 模式 | 涉及的 Agent | 所需配置原语 |
|------|-------------|-------------|
| **JSON 输出解析** | OpenClaw、Claude Code | `json_paths` — 有序 JSON 路径提取策略列表 |
| **纯文本输出** | ZeroClaw、NanoBot、PicoClaw、Hermes | `output_format: "text"` |
| **ANSI 剥离** | 全部（PicoClaw 关键） | 始终开启（通用） |
| **日志行过滤** | 全部 | `strip_patterns: [正则表达式]` |
| **`<think>` 标签剥离** | NanoBot、PicoClaw、Hermes | `strip_tags: ["think"]` |
| **🦞 分隔符** | PicoClaw | `delimiter: "🦞"`（提取最后一个之后的内容） |
| **JSON 会话提取** | OpenClaw、Claude Code | `session_json_path` |
| **文本会话提取** | Hermes | `session_text_pattern: "^session_id:\\s*(.+)"` |
| **共享角色文件** | OpenClaw、NanoBot、PicoClaw、Hermes | `role: { type: "shared", path: "..." }` |
| **独立角色文件** | Claude Code、ZeroClaw | `role: { type: "per_role", path: ".../{role_id}..." }` |
| **YAML name 提取** | 仅 Claude Code | `role_name_from: "yaml_frontmatter.name"` |
| **角色前置到消息** | NanoBot、Hermes | `role_injection: "message_prepend"` |
| **写入 JSON 配置** | OpenClaw、NanoBot、PicoClaw、Claude Code | `config_template` + `write_file` |
| **写入 TOML 配置** | ZeroClaw | `config_template` + `write_file`（TOML） |
| **CLI 命令配置** | Hermes | `config_commands: [...]` |
| **环境文件操作** | Hermes | `env_file: { path, set, remove }` |
| **额外文件写入** | Claude Code（`~/.claude.json`） | `setup_files: [{ path, content, condition }]` |
| **保留现有值** | OpenClaw（gateway token） | `preserve_keys: ["gateway"]` |
| **设置进程环境变量** | ZeroClaw | `process_env: { key: value }` |

---

## 5. 未来扩展场景

### 5.1 添加新 Agent
**当前**：在 6+ 个 `match agent_id` 分支 + `KNOWN_AGENTS` 中加代码 → 重编译 5 平台
**配置驱动**：在 `agents.json` 中加一条 → 零代码改动、零重编译

### 5.2 修复输出过滤
**当前**：在 `is_agent_log_line()` 加正则 → 重编译 5 平台（如本次 Hermes 方框修复）
**配置驱动**：编辑 `strip_patterns` 数组 → 随 app 更新推送

### 5.3 Agent 上游破坏性变更
**当前**：Agent 更新 CLI 参数或输出格式 → 必须更新 Bridge 代码 + 重编译
**配置驱动**：更新 `agents.json` 中的 command/args/json_paths → 随 app 更新推送

### 5.4 移动平台（Android/iOS）
**当前**：必须为 ARM Android + iOS 编译 Bridge（复杂的交叉编译）
**配置驱动**：Bridge 编译一次（通用），agent 逻辑全在配置文件中

---

## 6. 异常状态清单

| 异常 | 当前处理 | 状态 |
|------|---------|------|
| Agent 未安装 | `check_installed()` → 错误响应 | ✅ 已处理 |
| Agent 进程超时 | 远程：5 分钟超时；本地：用户可 Stop | ✅ 已处理 |
| 配置文件写入失败 | 各函数有 error 处理 | ✅ 已处理 |
| 角色下载失败 | HTTP 错误 → 错误响应 | ✅ 已处理 |
| 模型配置失败 | 各 Agent 有错误消息 | ✅ 已处理 |
| SSH 断开 | `stdin.lines()` 错误 → break | ✅ 已处理 |
| Agent 输出乱码 | 纯文本 fallback | ⚠️ 可改进但不紧急 |
| 环境变量冲突 | Hermes ANTHROPIC_API_KEY 已修复 | ✅ 已修复 |
| JSON/文本混合输出 | `find_json_object` 提取第一个 `{...}` | ⚠️ 低优先级 |
| Agent 上游破坏格式 | 只能重编译 | ⚠️ 配置驱动后可解决 |
| 配置文件编码 | `write_config_file()` 使用 UTF-8 | ✅ 已处理 |
| 并发访问 | 单线程 stdin 循环 | ✅ 无风险 |

---

## 7. 实现零 Agent 代码所需的 15 个配置原语

### 核心（覆盖 90% 逻辑）
1. **`exec`** — 执行命令，传递环境变量，捕获 stdout
2. **`filter_lines(patterns)`** — 按正则移除匹配行
3. **`parse_json(paths[])`** — 按优先级尝试多个 JSON 路径
4. **`parse_text(delimiter?, strip_tags[])`** — 提取文本 + 可选分隔符和标签剥离
5. **`capture(pattern)`** — 正则捕获 session_id 等

### 配置管理（覆盖模型切换）
6. **`write_file(path, template, format)`** — 写入 JSON/TOML/文本文件
7. **`run_command(cmd[])`** — 执行配置命令（如 `hermes config set`）
8. **`edit_env_file(path, set{}, remove[])`** — 增删 `.env` 文件条目
9. **`preserve_keys(path, keys[])`** — 读取现有文件，保留指定键
10. **`conditional_write(path, content, condition)`** — 仅在文件不存在时写入

### 角色管理
11. **`download_to(url, path_template)`** — 下载文件，支持 `{role_id}` 替换
12. **`yaml_extract(field)`** — 从 YAML frontmatter 提取字段
13. **`message_prepend(template)`** — 将角色注入消息文本

### 检测
14. **`check_command(name)`** — 测试命令是否存在于 PATH
15. **`check_process(name)`** — 测试进程是否在运行

---

## 8. 结论与建议

**零 Agent 代码是否可行？** 是的——所有 6 个 Agent 的逻辑都能用上述 15 个原语表达。

**当前建议（2026-03-23）：不做全面重构。** 理由：
- 6 个 Agent 数量不多，改动不频繁
- CI 自动编译 5 平台只需 ~15 分钟
- 重构投入约 2-3 周，ROI 不划算
- 现有异常处理已比较完善

**触发重构的时机：**
- 支持 **10+ 个 Agent** 时
- 开始做 **Android/iOS** 移动端时
- Agent 上游**频繁破坏兼容**时

**最大风险点：** Claude Code（YAML frontmatter + 多文件配置 + 独立角色文件）和 Hermes（CLI 命令 + .env 文件操作 + 旧条目清理）。
