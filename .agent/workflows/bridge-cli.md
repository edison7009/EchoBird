---
description: Architecture reference for Bridge CLI vs EchoBird main app — two separate programs
---

# Bridge CLI Architecture

> **核心认知：Bridge CLI 和 EchoBird 主程序是两个完全独立的程序。**

## 两个程序对照

| | EchoBird 主程序 | Bridge CLI |
|---|---|---|
| **类型** | Tauri 桌面应用 (Rust + React) | 纯 Rust 命令行工具 |
| **运行位置** | 用户本机 (Windows/macOS/Linux) | 远程服务器 (Linux 为主) |
| **源码路径** | `src-tauri/` + `src/` | `bridge/` |
| **编译产物** | EchoBird 安装包 (.exe/.dmg/.AppImage) | `bridge-*` 二进制 (~700KB) |
| **CI 产物** | `EchoBird_x64-setup.exe` 等 | `bridge-darwin-aarch64`, `bridge-linux-x86_64`, `bridge-win.exe` 等 |
| **Cargo.toml** | `src-tauri/Cargo.toml` | `bridge/Cargo.toml` |
| **依赖** | tauri, serde, tokio, uuid 等 | serde, serde_json, ureq (极简) |

## 通信协议

```
用户本机                          远程服务器
┌─────────────┐     SSH      ┌─────────────────┐
│  EchoBird   │ ──────────── │   Bridge CLI     │
│  (Tauri)    │  stdin/stdout│  (echobird-bridge)│
│             │   JSON lines │                   │
└─────────────┘              │  ↕ 调用 Agent CLI │
                             │  (openclaw/claude)│
                             └─────────────────┘
```

## Bridge CLI 的职责 (远程执行)

1. **聊天中继** — 接收 JSON → 调用 Agent CLI → 返回结果
2. **Agent 检测** — `detect_agents` → 检测远程已安装的 Agent
3. **角色管理** — `set_role` 下载角色文件 → 写入 agent 配置目录
4. **角色清除** — `clear_role` 删除角色文件
5. **Agent 启动** — `start_agent` 启动远程 Agent 进程
6. **状态查询** — `status` / `ping`

## EchoBird 主程序的职责 (本地执行)

1. **UI 渲染** — 频道列表、聊天界面、角色选择器
2. **SSH 管理** — 连接远程服务器、发送 JSON 命令
3. **本地 Agent 检测** — `detect_local_agents` (Tauri 后端)
4. **角色数据** — 扫描 `roles/roles-en.json` → 展示 UI
5. **持久化** — localStorage 保存 Agent/角色选择

## 编译 Bridge CLI

```powershell
# 本地编译（开发测试）
cd bridge
cargo build --release
# 产物：target/release/echobird-bridge(.exe)

# CI 自动编译（GitHub Actions）
# release.yml 自动为 5 个平台编译并上传到 Release Assets
```

## 常见错误提醒

> [!CAUTION]
> - **不要混淆** `src-tauri/Cargo.toml`（主程序）和 `plugins/openclaw/bridge/Cargo.toml`（Bridge CLI）
> - **不要把远程功能写到 Tauri 后端**——远程功能必须在 Bridge CLI 中实现
> - **Bridge CLI 不能用 tauri API**——它是独立二进制，没有 tauri 依赖
> - **Bridge CLI 尽量保持轻量**——部署到远程服务器，依赖越少越好
