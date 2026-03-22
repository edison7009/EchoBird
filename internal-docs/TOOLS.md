# Echobird 工具系统架构文档

> **重要**：每次修改工具相关功能前，必须先阅读本文档。

## 核心设计理念

**目录即工具**。每个工具是 `electron/tools/<tool-id>/` 下的一个目录，包含：

```
electron/tools/<tool-id>/
├── paths.json      # 必须 — 元数据 + 路径检测规则
├── config.json     # 必须 — 配置文件读写规则
└── model.cjs/.ts   # 可选 — 自定义模型读写逻辑（config.json 的 custom:true 时使用）
```

添加新工具 = 创建目录 + 填写 JSON。无需修改任何 TypeScript 代码。

---

## paths.json 规范

定义工具的**元数据**和**检测策略**。

```jsonc
{
    // === 元数据 ===
    "name": "OpenClaw",                    // 显示名称
    "category": "AgentOS",                 // 分类：AgentOS | IDE | CLI | AutoTrading
    "docs": "https://docs.openclaw.ai",    // 官网/文档链接
    "apiProtocol": ["openai", "anthropic"],// 支持的 API 协议
    "installUrl": "https://...",           // 安装地址（可选，用于 Install 按钮）

    // === 路径检测 ===
    "command": "openclaw",                 // CLI 命令名（通过 which/where 查找）
    "envVar": "OPENCLAW_PATH",             // 环境变量检测（可选）
    "configDir": "~/.openclaw",            // 配置目录
    "configFile": "~/.openclaw/openclaw.json", // 主配置文件路径
    "requireConfigFile": true,             // 是否要求配置文件存在才算已安装
    "detectByConfigDir": false,            // true = 通过配置目录判断安装（适用于 GUI 程序）

    // 平台特定的安装路径候选（~ 和 %VAR% 会被自动展开）
    "paths": {
        "win32": [
            "%APPDATA%/npm/openclaw.cmd",
            "%LOCALAPPDATA%/Programs/OpenClaw/openclaw.exe"
        ],
        "darwin": [
            "/usr/local/bin/openclaw",
            "/opt/homebrew/bin/openclaw",
            "/Applications/OpenClaw.app/Contents/MacOS/OpenClaw"
        ],
        "linux": [
            "/usr/local/bin/openclaw",
            "/usr/bin/openclaw"
        ]
    },

    // 技能目录检测（可选）
    "skillsPath": {
        "envVar": "OPENCLAW_SKILLS_PATH",
        "win32": ["路径"],
        "npmModule": "openclaw/skills"
    },

    // VS Code 扩展检测路径（可选，支持 * 通配符匹配版本号）
    "extensionPaths": {
        "win32": ["%USERPROFILE%/.vscode/extensions/publisher.extension-*"],
        "darwin": ["~/.vscode/extensions/publisher.extension-*"],
        "linux": ["~/.vscode/extensions/publisher.extension-*"]
    },

    // === 内置工具专用字段 ===
    "alwaysInstalled": true,               // 内置工具，始终视为已安装（跳过所有检测）
    "version": "1.0",                      // 内置工具版本号（无可执行文件时使用）
    "description": "Tool description",      // 工具描述（显示在卡片上）

    // === 可启动工具字段 ===
    "launchable": true,                    // 是否可通过 LAUNCH APP 按钮启动
    "launchType": "html",                  // 启动类型（目前支持 html）
    "launchFile": "game.html"              // 启动文件名（工具目录下的文件）
}
```

### 工具类型与启动方式

| 类型 | 示例 | `command` | `paths` | `detectByConfigDir` | 启动方式 |
|------|------|-----------|---------|---------------------|----------|
| **CLI 工具** | OpenClaw, Claude Code, Codex | ✅ `"openclaw"` | CLI 路径 (.cmd/.exe) | `false` | 终端命令 (`startCommand`) |
| **IDE 扩展** | Roo Code, Continue, Cline, Tabby | ❌ 不填或留空 | — | `true` | `shell.openPath()` |
| **GUI 桌面程序** | CodeBuddy, CodeBuddy CN | ❌ 不填或留空 | exe/app 路径 | `true` | `shell.openPath()` |
| **未安装工具** | TradingAgents, FinGPT | ❌ 不填或留空 | 空数组 `[]` | `false` | — |
| **内置工具** | Reversi | ❌ 不填或留空 | — | — | `launchGame()`（应用内弹窗） |

- **CLI 工具**：优先通过 `command` + `which/where` 查找，`paths` 作为备选。通过 `default-tools.json` 中的 `startCommand` 在终端启动
- **IDE 扩展**：通过 `extensionPaths` 通配符匹配 VS Code 扩展目录，或 `detectByConfigDir` 检测。通过 `shell.openPath(detectedPath)` 打开可执行文件（跨平台）
- **GUI 程序**：没有全局命令，通过 `paths` 中的 exe/app 路径 或 `detectByConfigDir` 检测。同样通过 `shell.openPath()` 启动
- **未安装工具**：只有元数据，前端显示 INSTALL 按钮
- **内置工具**：`alwaysInstalled: true`，跳过所有检测，返回工具目录路径。可配合 `launchable` 实现应用内启动

### 三平台路径规范

路径中支持的变量会在检测时自动展开：

| 变量 | Windows 展开 | macOS/Linux 展开 |
|------|-------------|-----------------|
| `~` | `C:\Users\<user>` | `/Users/<user>` 或 `/home/<user>` |
| `%APPDATA%` | `C:\Users\<user>\AppData\Roaming` | — |
| `%LOCALAPPDATA%` | `C:\Users\<user>\AppData\Local` | — |
| `%USERPROFILE%` | `C:\Users\<user>` | — |

#### 常见路径模式

**Windows (win32)**
```
CLI 全局:  %APPDATA%/npm/<tool>.cmd
CLI Scoop: %USERPROFILE%/scoop/shims/<tool>.exe
GUI 程序:  %LOCALAPPDATA%/Programs/<AppName>/<App>.exe
```

**macOS (darwin)**
```
CLI Homebrew: /opt/homebrew/bin/<tool>
CLI 系统:     /usr/local/bin/<tool>
GUI 程序:     /Applications/<App>.app/Contents/MacOS/<App>
```

**Linux (linux)**
```
CLI 全局: /usr/local/bin/<tool>
CLI 系统: /usr/bin/<tool>
```

### 检测优先级

0. `alwaysInstalled` → 内置工具直接返回工具目录的绝对路径（跳过后续所有步骤）
1. `envVar` 环境变量
2. `command` 通过 which/where 查找
3. `paths[platform]` 固定路径列表（按顺序逐个检查）
4. `detectByConfigDir` → 配置目录存在性判断（GUI/IDE 程序专用）
5. `extensionPaths` → VS Code 扩展目录通配符匹配
6. `requireConfigFile` → 配置文件存在性校验（在步骤 2、3 中作为附加条件）

---

## config.json 规范

定义如何**读写**工具的配置文件。有两种模式：

### 模式 1：字段映射（简单工具）

```jsonc
{
    "configFile": "~/.opencode/config.json",
    "format": "json",                      // json | yaml | toml
    "read": {                              // 配置路径 → ModelInfo 字段
        "providers.openai.apiKey": "apiKey",
        "providers.openai.model": "model"
    },
    "write": {                             // 配置路径 → ModelInfo 字段
        "providers.openai.apiKey": "apiKey",
        "providers.openai.model": "model",
        "providers.openai.baseUrl": "baseUrl"
    },
    "writeProxy": {                        // 代理配置（可选）
        "network.proxy": "proxyUrl"
    }
}
```

### 模式 2：自定义模块（复杂工具）

```jsonc
{
    "configFile": "~/.openclaw/openclaw.json",
    "format": "json",
    "custom": true                         // 启用 model.cjs 自定义逻辑
}
```

当 `custom: true` 时，loader 会加载同目录下的 `model.cjs`，该模块必须导出：

```javascript
module.exports = {
    // 读取当前模型配置
    async getCurrentModelInfo(readConfig) { ... },
    // 写入模型配置
    async applyConfig(modelInfo, readConfig, writeConfig, getConfigFile) { ... }
};
```

---

## model.cjs 自定义模块规范（⚠️ 仅 Electron 旧版）

> **Tauri 版不再使用 model.cjs**。所有配置逻辑已迁移到 Rust `tool_config_manager.rs` 中的专用函数。以下内容保留作为参考。

### 入参

- `modelInfo`：来自 Echobird 的模型信息
  ```typescript
  interface ModelInfo {
      id: string;        // 内部通信 ID
      name: string;      // 显示名称
      model: string;     // API 模型 ID（如 deepseek-chat）
      baseUrl: string;   // API 地址
      apiKey: string;    // API Key
      proxyUrl?: string; // 代理地址
  }
  ```
- `readConfig`：读取工具配置文件，返回解析后的对象
- `writeConfig`：写入工具配置文件，返回 boolean
- `getConfigFile`：获取配置文件路径

### 关键规则

1. **始终只保留一条模型配置**：每次写入时将模型列表替换为只含当前模型的数组，而非追加。这确保用户始终拥有清爽的配置，不会因反复切换模型而累积垃圾数据。示例：`config.models = [newModel]`
2. **不写入工具不认识的字段**：严格遵守目标工具的配置格式（OpenClaw 的教训）
3. **vendor 从 URL 提取**：`api.deepseek.com` → `deepseek`（如果目标工具需要 vendor 字段）
4. **声明式 write 映射天然安全**：使用固定配置路径覆盖（如 `"providers.openai.model": "model"`），不存在累积问题

---

## 数据流（Tauri/Rust 版）

```
用户在前端选择模型 → 点击 MODIFY ONLY
    ↓
App.tsx: invoke("apply_model_to_tool", { toolId, modelInfo })
    ↓
Tauri Command → tool_config_manager.rs::apply_model_to_tool()
    ↓
match tool_id {
    "cline"|"roocode"|"openclaw" → apply_echobird_relay()  + run_patch_script()
    "easyclaw"                   → apply_easyclaw()         (直写 ~/.easyclaw/easyclaw.json)
    "codebuddy"|"codebuddycn"   → apply_codebuddy()
    "workbuddy"                  → apply_codebuddy()        (~/.workbuddy/models.json)
    "opencode"                   → apply_opencode()
    "aider"                      → apply_aider()           (YAML)
    "continue"                   → apply_continue_dev()     (YAML)
    "codex"                      → apply_codex()            (TOML + echobird JSON)
    "zeroclaw"                   → apply_zeroclaw()         (TOML)
    _                            → apply_generic_json()     (config.json 映射)
}
    ↓
写入工具配置文件 → 运行补丁脚本（如有）
```

> **注意**：旧版 Electron 使用 `loader.ts` + `model.cjs` 动态加载模块。Tauri 版改为 Rust 硬编码 `match` 分发，不再依赖 `model.cjs`/`model.ts`。

---

## 工具扫描流程（Tauri/Rust 版）

```
tool_manager.rs::scan_tools()
    ↓
1. find_tools_dir() → 定位 electron/tools/ 目录
2. 遍历子目录：读取 paths.json → 解析为 ToolDefinition
3. 逐个 detect() → 多策略检测安装状态
   ├── alwaysInstalled → 直接返回
   ├── command → which/where 查找
   ├── paths[platform] → 逐个检查路径
   ├── detectByConfigDir → 配置目录存在性
   └── extensionPaths → VS Code 扩展匹配
4. tool_config_manager.rs::get_tool_model_info() → 读取当前模型
5. 返回 Vec<DetectedTool> 给前端（通过 Tauri command）
```

---

## 添加新工具清单（Tauri 版）

1. 创建 `electron/tools/<tool-id>/` 目录
2. 编写 `paths.json`（元数据 + 路径检测）
3. 编写 `config.json`（配置读写规则）
4. 如果工具的配置格式被 `apply_generic_json` 支持（标准 JSON 字段映射），则无需额外 Rust 代码
5. 如果工具需要特殊配置格式（YAML/TOML/自定义 JSON），需要在 `tool_config_manager.rs` 中：
   - 添加 `apply_<tool>()` 和 `read_<tool>()` 函数
   - 在 `apply_model_to_tool()` 和 `get_tool_model_info()` 的 `match` 中添加分支
6. 如果工具需要补丁注入，编写 `patch-<tool>.cjs`
7. 如果工具需要代理/启动器，编写 `<tool>-launcher.cjs`
8. 重新编译 Rust 后端（`cargo build`）→ 工具自动出现在前端

> **对比旧版**：Electron 版只需写 `model.cjs` + 设 `custom: true`，Tauri 版需要在 Rust 中添加代码（但大多数工具走 `apply_generic_json` 不需要）。

---

## 未来规划：用户自定义工具

用户将可以通过前端 UI 添加自定义工具：
- 添加本地应用程序/终端命令
- 通过 Logs & Debug 页面的 AI 辅助配置
- AI 根据本文档的规则自动生成 paths.json 和 config.json
- 前提：我们自己的规则必须清晰完善

---

## 补丁注入方案（Patch Injection）

> 适用于：Cline、Roo Code 等不支持纯配置文件方式的 VS Code 扩展

### 通用原理

某些 VS Code 扩展（如 Cline、Roo Code）将 API Key 存储在 VS Code 的 globalState / SecretStorage 中，
无法通过简单的 JSON 配置文件修改。解决方案是在扩展的 `extension.js` 中注入代码，
启动时从 `~/.echobird/<tool>.json` 读取外部配置，覆盖内部缓存。

### 文件结构

```
electron/tools/<tool>/
├── paths.json          # 元数据 + 扩展路径检测
├── config.json         # custom: true（使用 model.cjs）
├── model.ts → model.cjs  # Apply 时写入配置文件 + 自动打补丁
└── patch-<tool>.cjs    # 补丁脚本，修改 extension.js
```

### 工作流程（Tauri 版）

```
用户点击 Apply → tool_config_manager.rs::apply_echobird_relay()
  ├── 1. 写入 ~/.echobird/<tool>.json（API Key、模型 ID、Base URL）
  ├── 2. run_patch_script() 执行 patch-<tool>.cjs
  │     ├── 已打补丁（检测 PATCH_MARKER）→ 跳过
  │     └── 未打补丁 → 备份并注入代码
  └── 3. 返回结果，提示用户重启 VS Code

VS Code 启动 → 扩展加载 extension.js
  └── 注入代码读取 ~/.echobird/<tool>.json → 覆盖 stateCache / secretCache
```

> **对比旧版**：Electron 使用 `model.cjs` + `ensurePatch()`，Tauri 使用 `apply_echobird_relay()` + `run_patch_script()`。

### 补丁脚本规范（patch-\<tool\>.cjs）

```bash
# 打补丁
node patch-<tool>.cjs

# 恢复原始文件
node patch-<tool>.cjs --restore
```

**关键要素：**
- `PATCH_MARKER`：唯一标记（如 `[Echobird-Patched]`），用于检测是否已打补丁
- 自动备份原始 `extension.js`（`.echobird-backup` 后缀）
- 重复打补丁时先从 backup 恢复再重新注入
- 注入代码用 IIFE + try/catch 包裹，失败时静默降级

### Cline 具体实现

**注入点**：`extension.js` 中 `.populateCache(r,n,o),` 之后

**配置文件**：`~/.echobird/cline.json`

```json
{
  "provider": "openai",
  "apiKey": "sk-xxx",
  "baseUrl": "https://api.example.com/v1",
  "modelId": "gpt-4o",
  "modelName": "GPT-4o"
}
```

**字段映射（Cline 3.61.0+，per-mode provider）**：

| Cache | 字段名 | 值 |
|-------|--------|-----|
| globalStateCache | `actModeApiProvider` | `"openai"` |
| globalStateCache | `planModeApiProvider` | `"openai"` |
| globalStateCache | `actModeOpenAiModelId` | Model ID |
| globalStateCache | `planModeOpenAiModelId` | Model ID |
| globalStateCache | `openAiBaseUrl` | API Base URL |
| globalStateCache | `actModeOpenAiModelInfo` | `{ maxTokens, contextWindow, ... }` |
| globalStateCache | `planModeOpenAiModelInfo` | (same) |
| secretsCache | `openAiApiKey` | API Key |

> ⚠️ Cline 3.61.0 改为 per-mode provider：`actModeApiProvider` / `planModeApiProvider`，旧版 `apiProvider` 已失效。

**字段来源**：`cline/cline` → `src/shared/storage/state-keys.ts`

### Roo Code 具体实现

**注入点**：`extension.js` 中 `this._isInitialized=!0` 之前（StateManager.initialize() 末尾）

**配置文件**：`~/.echobird/roocode.json`

```json
{
  "apiKey": "sk-xxx",
  "baseUrl": "https://api.example.com/v1",
  "modelId": "model-name",
  "modelName": "Model Name"
}
```

**字段映射**：

| 写入目标 | 字段名 | 值 |
|----------|--------|-----|
| stateCache | `apiProvider` | `"openai"` |
| stateCache | `openAiModelId` | Model ID |
| stateCache | `openAiBaseUrl` | API Base URL |
| secretCache | `openAiApiKey` | API Key |
| originalContext.globalState | apiProvider / openAiModelId / openAiBaseUrl | (持久化) |
| originalContext.secrets | openAiApiKey | (持久化到 VS Code SecretStorage) |

> ⚠️ Roo Code（Cline fork）需要**双重写入**：内存缓存 + VS Code 存储 API。仅写内存缓存会导致 API Key 丢失。

### 不适用补丁的工具

以下工具使用封闭式服务器代理模式，无法通过本地补丁集成：
- **Cursor** — AI 请求通过 `api2.cursor.sh` 代理，API Key 绑定 Cursor Pro 账号
- **GitHub Copilot** — AI 请求通过 GitHub API 代理，API Key 绑定 Copilot 订阅

### 注意事项

1. Apply 后必须**重启 VS Code** 才能生效（注入代码仅在扩展加载时执行一次）
2. 扩展更新后补丁自动重打（`ensurePatch()` 每次 Apply 时检测）
3. Cline 仅支持 OpenAI Compatible（Anthropic 不支持自定义 base URL）
4. Roo Code 仅支持 OpenAI Compatible（同上）
5. 有 `extensionPaths` 的工具应设 `detectByConfigDir: false`，避免卸载后配置目录残留导致误判

---

## OpenClaw 模型配置方案

> 适用于：OpenClaw v2026.3.13+（npm 全局安装的 CLI 工具）

### 方案演进

| 版本 | 方案 | 状态 |
|------|------|------|
| v2026.3.11 之前 | 补丁注入 `openclaw.mjs` → 启动时读 `~/.echobird/openclaw.json` → 写回 `openclaw.json` | ❌ 已废弃 |
| v2026.3.11 | 补丁注入（更新 schema：移除 auth/authHeader，添加 input/reasoning） | ❌ 已废弃 |
| v2026.3.13 初版 | 直接 merge-write `~/.openclaw/openclaw.json`（保留旧配置，合并新 provider） | ❌ 已废弃 |
| **v2026.3.13+** | **全新覆盖 `~/.openclaw/openclaw.json`（仅保留 gateway token）** | ✅ 当前方案 |

### 工作流程（Tauri 版，v2026.3.13+）

不再补丁 `openclaw.mjs`，**全新覆盖** `~/.openclaw/openclaw.json`。
之前用 merge-write 保留旧配置容易残留脏数据，现在一律覆盖，仅保留 `gateway` token。

```
App Manager / Channels 选模型 → 调用 apply_openclaw() 或 bridge_set_local_model("openclaw")
  ├── 1. 读取旧 ~/.openclaw/openclaw.json（仅提取 gateway token）
  ├── 2. 生成全新的 JSON 配置（eb_<providerTag> provider + models.mode="merge" + agents.defaults）
  ├── 3. 写回 gateway token（如存在）
  ├── 4. 全新覆盖写入 ~/.openclaw/openclaw.json
  └── 5. 同步写 ~/.echobird/openclaw.json relay（供 Channels 页面读取）

远程频道选模型 → bridge_set_remote_model("openclaw")
  └── SSH 执行同样逻辑：全新覆盖远程 ~/.openclaw/openclaw.json

Bridge 发消息 → openclaw agent --json --agent main --message "..."
  └── OpenClaw 读取 ~/.openclaw/openclaw.json → 使用 eb_xxx/model-id

读取当前模型 → bridge_get_local_model("openclaw")
  └── 直接从 ~/.openclaw/openclaw.json 解析 agents.defaults.model.primary
```

### 为什么从补丁切到直接覆盖

补丁方案是**迫不得已**的选择：老版本 OpenClaw 没有公开稳定的自定义 provider 格式，
写进去的配置会被 OpenClaw 启动时覆盖或不认。所以才注入 `.mjs` 入口，在启动瞬间写入。
v2026.3.13 官方文档明确了 `models.providers` + `mode: "merge"` 格式，我们可以直接写了。

### Channels 模型选择的细节

1. **协议过滤**：Claude Code 只支持 Anthropic 协议 → 模型列表仅显示有 `anthropicUrl` 的模型
2. **baseUrl/protocol 匹配**：有 `anthropicUrl` 的模型用 `anthropicUrl` + `'anthropic'`，否则用 `baseUrl` + `'openai'`
3. **model ID**：传 `modelId`（API 真实模型名如 `MiniMax-M2.7`），不是 `internalId`（EchoBird 内部 ID 如 `m-198c5a`）
4. **模型同步**：用户从 App Manager 改模型后返回 Channels，`window.focus` 事件自动重新读取当前模型

### 配置格式（v2026.3.13+）

```jsonc
// protocol = "openai" → api: "openai-completions"
// protocol = "anthropic" → api: "anthropic-messages"
{
  "models": {
    "mode": "merge",                           // ← v2026.3.13 新增：与内置 provider 合并
    "providers": {
      "eb_minimaxi": {
        "baseUrl": "https://api.minimaxi.com/v1",
        "apiKey": "sk-xxx",
        "api": "openai-completions",           // 或 "anthropic-messages"
        "models": [{
          "id": "MiniMax-M2.7",
          "name": "MiniMax M2.7",
          "contextWindow": 128000,
          "maxTokens": 8192,
          "input": ["text"],                   // ← v2026.3.11+ 必填
          "reasoning": false,                  // ← v2026.3.11+ 必填
          "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        }]
      }
    }
  },
  "agents": { "defaults": { "model": { "primary": "eb_minimaxi/MiniMax-M2.7" } } }
}
```

### 踩坑记录

> ⚠️ 以下是调试过程中实际遇到的问题，务必注意。

1. **不能用 OpenClaw 内置 Anthropic provider**
   - OpenClaw 内置 `anthropic/` 前缀只认 Claude 系列模型名（`claude-sonnet-4-20250514` 等）
   - 非 Claude 模型（如 `mimo-v2-flash`）用 `anthropic/` 前缀会报 `Unknown model`
   - 解决：统一走自定义 provider `eb_xxx`，通过 `api: "anthropic-messages"` 区分协议

2. **OpenClaw 严格校验 JSON Schema**
   - 自定义 provider 中不能有 OpenClaw 不认识的字段（如我们加的 `_Echobird: true`）
   - 会导致 `Unrecognized key` 错误，配置被判定为 invalid
   - 解决：用 `eb_` 命名前缀识别 Echobird 推送的 provider，不加额外标记字段

3. **v2026.3.11 schema 破坏性变更**
   - 移除了 `auth` 和 `authHeader` 字段
   - model 对象必须包含 `input: ["text"]` 和 `reasoning: false` 字段
   - 旧格式写入后被 schema 校验**静默丢弃**

4. **v2026.3.13 补丁注入点失效**
   - `await installProcessWarningFilter();` 在 v2026.3.13 的 `openclaw.mjs` 中**不再存在**
   - 解决：弃用补丁注入，改为直接写入 `openclaw.json`（当前方案）
   - `tool_patcher.rs` 中的 `OPENCLAW_INJECT` 和 `patch_openclaw()` 不再被调用

5. **`models.mode: "merge"` 是关键**
   - v2026.3.13 需要 `models.mode: "merge"` 才能与内置 provider 合并
   - 不设此字段可能导致自定义 provider 被忽略

6. **`auth-profiles.json` 是新的认证系统**
   - OC v2026.3.13 内置 provider（如 anthropic、openai）使用 `~/.openclaw/agents/main/agent/auth-profiles.json`
   - 我们的自定义 `eb_` provider 直接在 `models.providers` 中嵌入 apiKey，**不依赖 auth-profiles.json**

---

## Claude Code (cli-oneshot)

> Echobird v3.0.10+ | Protocol: cli-oneshot (one CLI invocation per message, no persistent subprocess)

### Configuration

- **Config file**: `~/.claude/settings.json`
- **Model config**: `env.ANTHROPIC_MODEL`, `env.ANTHROPIC_BASE_URL`, `env.ANTHROPIC_API_KEY`
- **APP page**: Uses generic JSON mapping (`tools/claudecode/config.json`), no custom handler needed
- **Role files**: `~/.claude/agents/{role_id}.md` (per-role files, NOT shared like OpenClaw's SOUL.md)

### First-time setup (CRITICAL)

Claude Code requires **TWO config files** for non-interactive use:

**Step 1: `~/.claude/config.json`** — Skip login/onboarding (MUST be first!)

Without this, Claude Code opens an interactive login flow that **hangs automated sessions**.

```json
{
    "hasCompletedOnboarding": true,
    "primaryApiKey": "dummy"
}
```

**Step 2: `~/.claude/settings.json`** — Tool permissions + model config

Without `allowedTools`, Claude Code prompts for folder trust and tool approvals interactively.

```json
{
    "allowedTools": ["Edit","Write","Bash","Read","MultiEdit","Glob","Grep","LS","TodoRead","TodoWrite","WebFetch","NotebookRead","NotebookEdit"],
    "env": {
        "ANTHROPIC_MODEL": "claude-sonnet-4-20250514",
        "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
        "ANTHROPIC_API_KEY": "sk-ant-..."
    }
}
```

### Echobird integration (via Bridge — unified for all agents)

| Component | How it works |
|-----------|-------------|
| Plugin config | `plugins/claudecode/plugin.json` — `-p --dangerously-skip-permissions --output-format json` |
| Role loading | Bridge `set_role` downloads `.md` to `~/.claude/agents/{role_id}.md`, then `--agent {YAML name}` |
| Role name | Claude Code matches by YAML frontmatter `name:` field, NOT filename. Frontend passes `role.name` as `agent_name` |
| Session resume | `resumeArgs` in plugin.json uses `--resume {sessionId}` (NOT `--session-id`) |
| JSON parsing | Bridge `parse_agent_output()` handles Claude Code's JSON format |

### Pitfalls (learned from integration)

1. **`--agent` uses YAML `name`, not filename** — `narrative-designer.md` has `name: 叙事设计师`, so `--agent 叙事设计师` works but `--agent narrative-designer` doesn't
2. **`--resume` ≠ `--session-id`** — `--resume` continues existing session, `--session-id` creates NEW session with specific UUID
3. **Bridge is the sole communication layer** — role download / chat / session all go through Bridge (local persistent subprocess or remote one-shot)
4. **`&a[..30]` panics on Chinese** — always use `safe_truncate()` for multi-byte UTF-8
5. **Role dedup key needs agent ID** — use `${agentId}:${role.id}` to avoid stale role when switching agents

---

## Codex (Responses to Chat Proxy)

> 适用于：Codex CLI（本地安装的编码代理工具）

### 问题背景

Codex 新版**只支持 `wire_api = "responses"`**（`/v1/responses` 格式），已移除 `chat` 支持。
但所有第三方模型（DeepSeek、Qwen、MiniMax 等）**只支持 `/v1/chat/completions`**。

由于 Codex 的 API 请求逻辑在**编译好的 Rust 二进制文件**（`codex.exe`）中，无法通过 JS 补丁修改请求格式。

### 解决方案：本地代理双向格式转换

```
Codex（发 /v1/responses）
    ↓
Echobird 本地代理（127.0.0.1:自动端口）
    ├── 请求：Responses → Chat Completions
    ├── 响应：Chat Completions → Responses
    └── 流式：Chat SSE → Responses SSE
    ↓
第三方 API（收 /v1/chat/completions）
```

**Codex 以为在跟 OpenAI 对话，第三方 API 以为收到了标准请求。两边都被"骗"了。**

### 核心文件（Tauri 版）

| 文件 | 用途 |
|------|------|
| `tools/codex/codex-launcher.cjs` | 自包含启动器：内含代理逻辑 + 异步 spawn Codex |
| `src-tauri/.../tool_config_manager.rs` | `apply_codex()`：写入 config.toml（`wire_api = "responses"`）+ `~/.echobird/codex.json` |
| `src-tauri/.../process_manager.rs` | Priority 1.5：检测 launcher 并用 `node codex-launcher.cjs` 启动 |

> **旧版**（Electron）使用 `responsesProxy.ts` + `model.ts`，代理在 Electron 主进程内运行。

### 工作流程（Tauri 版）

```
用户点击 Apply → tool_config_manager.rs::apply_codex()
  ├── 写入 ~/.codex/config.toml（model、base_url、wire_api="responses"）
  ├── 写入 ~/.echobird/codex.json（apiKey、baseUrl、modelId）
  └── ⚠️ wire_api 始终为 "responses"，Codex v0.107+ 已移除 "chat" 支持
  └── run_patch_script("codex")（如有 patch-codex.cjs）

用户点击 Launch → process_manager.rs::start_tool("codex")
  ├── Priority 1.5：检测到 tools/codex/codex-launcher.cjs 存在
  └── 执行 node codex-launcher.cjs（CREATE_NEW_CONSOLE 独立终端窗口）

codex-launcher.cjs 执行流程：
  ├── 1. 读取 ~/.echobird/codex.json
  ├── 2. 判断 baseUrl 是否为 api.openai.com
  │     ├── 是 OpenAI → 直连，设 OPENAI_API_KEY
  │     └── 非 OpenAI → 启动本地代理
  │           ├── startProxy(realBaseUrl, apiKey) → 获得端口
  │           ├── 重写 ~/.codex/config.toml base_url → http://127.0.0.1:<port>/v1
  │           └── 设 OPENAI_API_KEY + OPENAI_BASE_URL
  └── 3. spawn codex（继承 stdio，用户看到 Codex TUI）

Codex 运行时 → 请求 http://127.0.0.1:<port>/v1/responses
  └── 代理收到 → responsesToChat() 转换 → 发给第三方 API /chat/completions
      └── 第三方 API 响应 → chatToResponses() 转换 → 返回 Codex
```

### 格式映射

**请求转换**：

| Responses API 字段 | Chat Completions 映射 |
|------|------|
| `instructions` | `messages[0] = {role: "system", content: ...}` |
| `input`（字符串） | `messages[n] = {role: "user", content: ...}` |
| `input`（数组） | 遍历 item，按 type/role 映射到 messages |
| `max_output_tokens` | `max_tokens` |
| `temperature` | `temperature` |

**流式 SSE 事件映射**：

| Chat Completions SSE | → 转换为 Responses SSE |
|-----|-----|
| 首次连接 | `response.created` → `response.in_progress` → `output_item.added` → `content_part.added` |
| `delta.content` | `response.output_text.delta` |
| `[DONE]` | `output_text.done` → `content_part.done` → `output_item.done` → `response.completed` |

### 与补丁方案的对比

| 对比项 | 补丁方案（Cline/OpenClaw） | 诈骗方案（Codex） |
|--------|--------------------------|-------------------|
| 修改对象 | 工具内的 JS 源码 | 不修改任何代码 |
| 原理 | 注入代码覆盖配置 | 本地代理格式转换 |
| 升级影响 | 工具升级后需重新打补丁 | 完全不受影响 |
| 适用场景 | API 格式兼容的情况 | API 格式不兼容的情况 |
| 已知限制 | — | 不支持 Codex 的工具调用（function_call） |

### 踩坑记录

> ⚠️ 以下是调试过程中实际遇到的问题，务必注意。

1. **~~esbuild `bundle: false` 问题~~**（仅 Electron 旧版，Tauri 不适用）
   - Tauri 版不使用 esbuild 编译工具脚本，`codex-launcher.cjs` 直接由 Node.js 运行
   - 代理逻辑直接内嵌在 `codex-launcher.cjs` 中，无跨文件 import 问题

2. **`wire_api = "chat"` 已被 Codex v0.107+ 移除**
   - Codex v0.107+ 明确报错：`'wire_api = "chat"' is no longer supported`
   - 所有 provider 必须用 `wire_api = "responses"`，第三方 API 通过代理转换格式
   - 解决：`apply_codex()` 始终写入 `wire_api = "responses"`

3. **代理端口动态分配**
   - 代理用 `server.listen(0)` 让系统自动分配空闲端口
   - 不要用固定端口，避免与应用自带的代理（localModelHandlers）冲突
   - 端口号写入 config.toml 的 `base_url = "http://127.0.0.1:<port>/v1"`

4. **Codex 二进制不可修改**
   - 不同于 Cline/OpenClaw 的 JS 入口，Codex 的 API 逻辑在编译好的 Rust 二进制 `codex.exe` 中
   - 无法通过补丁修改请求格式，只能用代理方案"骗"
   - 这也是为什么诈骗方案比补丁方案更适合 Codex

5. **`developer` 角色不被第三方 API 识别**
   - OpenAI 新版 API 用 `developer` 角色替代 `system`
   - Codex 发送的消息里会包含 `role: "developer"`
   - 第三方 API（如 MiniMax、DeepSeek）不认这个角色，返回 400 错误
   - 解决：代理中将 `developer` 映射为 `system`

6. **部分 API 完全不支持 `system` 角色**
   - MiniMax M2.5 也不认 `system` 角色，返回 `invalid message role: system`
   - 解决：将 `system` 消息内容合并到下一条 `user` 消息中，加 `[System Instructions]` 前缀
   - 这样所有 API 都只会收到 `user` + `assistant` 角色，最大兼容性

7. **流式响应必须检查 statusCode**
   - 上游返回非 200 时（如 401/400），若直接进入流式处理会静默返回空文本
   - Codex 收到空的 `response.completed`，表现为"没回复"（无报错）
   - 解决：进入流式/非流式处理前统一检查 statusCode，非 200 直接返回错误正文

8. **本地模型的 `modelId` 可能为空**
   - 云端模型有明确的 `modelId`（如 `deepseek-chat`），但本地模型没有
   - Codex 显示的模型名来自 config.toml 的 `model` 字段
   - 解决：`model = modelInfo.model || modelInfo.name || 'unknown'` 做 fallback

9. **`spawnSync` 阻塞 Node.js 事件循环**
   - `codex-launcher.cjs` 中用 `spawnSync` 启动 Codex，会阻塞整个进程
   - 代理服务器和 Codex TUI 在同一进程中——`spawnSync` 让代理无法处理请求
   - 表现：代理启动后 Codex 挂起，请求超时无回复
   - 解决：改用异步 `spawn` + `shell: true`，事件循环保持活跃
   - Codex 退出时通过 `child.on('close')` 清理代理并恢复 config.toml

10. **`startCommand` 跳过 launcher（Priority 顺序问题）**
    - `paths.json` 定义了 `startCommand: "codex"`，前端传给后端
    - 后端 Priority 1 匹配到 `startCommand`，直接 `start_cli_tool("codex")`
    - **完全跳过了 Priority 1.5 的 launcher 检测**——代理从未启动
    - Claude Code 不受影响（不需要代理）
    - 解决：Codex launcher 提升为 **Priority 0**，在 `startCommand` 检查之前执行

11. **`split_whitespace` 保留引号导致路径错误**
    - `format!("node \"{}\"", path)` 生成带引号的命令字符串
    - `start_cli_tool` 用 `split_whitespace` 拆分 → 引号成为文件名的一部分
    - `Command::new("node").args(["\"path\""])` → node 找不到文件
    - 解决：新增 `start_codex_launcher()` 方法，直接用 `Command::new("node").arg(path)` 传路径

12. **App Manager 模型列表不刷新**
    - `AppManagerProvider` 始终挂载（CSS hidden 切换页面）
    - `useEffect([], [])` 只在首次挂载加载模型列表
    - 用户在 Model Nexus 添加模型后切回 App Manager，列表不更新
    - 解决：传入 `isActive={activePage === 'apps'}` 作为 useEffect 依赖

13. **Windows debug 构建的 `\\?\` 扩展路径前缀导致启动失败**
    - Rust/Tauri 在 debug 模式下，`PathBuf` 转字符串（`to_string_lossy()`）可能返回带 `\\?\` 前缀的路径（Windows 扩展长路径格式），例如 `\\?\D:\build-output\debug\_up_\tools\codex\codex-launcher.cjs`
    - `cmd.exe` **不认识** `\\?\` 前缀，会将其误解析为相对路径，产生形如 `C:\Users\eben\?\D:\...\codex-launcher.cjs` 的无效路径
    - 另一个陷阱：把路径拼进字符串（`format!("node \"{}\"", path)`）再传给 `cmd /C`，node 会收到**带字面引号**的模块名，同样找不到文件
    - 解决：`start_codex_launcher()` 中先用 `strip_prefix(r"\\?\")` 去除扩展前缀，再用 `cmd.args(["/C", "node", launcher_clean])` 将路径作为**独立参数**传递，完全绕开引号拼接问题

### 验证结果

| 模型类型 | 示例 | 状态 |
|---------|------|------|
| 第三方云端 | xiaomimo (mimo-v2-flash)、DeepSeek (deepseek-chat) | ✅ 正常 |
| 第三方云端（思维链） | MiniMax (MiniMax-M2.5) | ✅ 正常（输出含 think 标签） |
| 本地模型 | qwen2.5-coder-1.5b-instruct | ✅ 正常 |
| OpenAI 官方 | 直连不走代理 | ✅ 设计如此 |

