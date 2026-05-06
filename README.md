<p align="center">
  <img src="docs/icon.png" alt="百灵鸟" width="120" />
</p>

<h1 align="center">百灵鸟 / EchoBird</h1>

<p align="center"><strong>AI 部署不再是:先有鸡,还是先有蛋</strong></p>
<p align="center"><sub>AI deployment, no more chicken-and-egg.</sub></p>

很多朋友让我帮他们安装 **Claude Code**、**OpenClaw**、**Hermes Agent**……不但每个人的系统都不一样，甚至有些人还抠门到不愿花钱买大模型，安装和解释起来都特别费劲。于是我开发了这个叫「百灵鸟」的 Agent —— 灵感来自《赛博朋克 2077》里那位总能帮主角搞定一切技术难题的女黑客。百灵鸟可以帮你一键装好所有 AI 工具。

> Friends kept asking me to install **Claude Code**, **OpenClaw**, **Hermes Agent**… every machine was different, and some refused to pay for an LLM. Setup and explanations took forever. So I built **EchoBird** — an Agent inspired by the female hacker in *Cyberpunk 2077* who fixes any tech problem for V. EchoBird installs every AI tool with one click.

<p align="center">
  <a href="https://github.com/edison7009/EchoBird/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/EchoBird?style=flat-square&color=D97757" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/badge/built%20with-Tauri%20%2B%20Rust-orange?style=flat-square" alt="Tauri + Rust" />
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="MIT License" />
</p>

<p align="center">
  <a href="https://echobird.ai">官网</a> ·
  <a href="https://github.com/edison7009/EchoBird/releases/latest">下载</a>
</p>

---

## 安装 / Install

### 一行命令安装 / One-line Install

**Windows** (PowerShell)

```powershell
irm https://echobird.ai/install.ps1 | iex
```

**macOS / Linux**

```sh
curl -fsSL https://echobird.ai/install.sh | sh
```

脚本会自动检测系统、下载对应安装包、跳过已安装的最新版本。
The script auto-detects your OS, downloads the right package, and skips if you're already on the latest version.

### 或下载安装包 / Or download a package

最新版本 / Latest release → <https://github.com/edison7009/EchoBird/releases/latest>

| 平台 / Platform | 文件 / Asset |
|---|---|
| Windows x64 | `EchoBird_<ver>_Windows_x64-setup.exe` |
| macOS (Apple Silicon) | `EchoBird_<ver>_macOS_arm64.dmg` |
| Linux x64 · Debian/Ubuntu | `EchoBird_<ver>_Linux_x64.deb` |
| Linux arm64 · Debian/Ubuntu | `EchoBird_<ver>_Linux_arm64.deb` |
| Linux x64 · Fedora/RHEL | `EchoBird_<ver>_Linux_x64.rpm` |
| Linux arm64 · Fedora/RHEL | `EchoBird_<ver>_Linux_arm64.rpm` |

---

<p align="center">
  Made with 💚 by 百灵鸟团队<br>
  <sub>⭐ <a href="https://github.com/edison7009/EchoBird">Star on GitHub</a></sub>
</p>
