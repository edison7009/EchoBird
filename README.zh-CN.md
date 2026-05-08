<p align="center">
  <img src="docs/icon.png" alt="EchoBird" width="140" />
</p>

<h1 align="center">EchoBird</h1>

<p align="center"><strong>AI 部署,不再是先有鸡还是先有蛋。</strong></p>

<p align="center">
  <a href="https://github.com/edison7009/EchoBird/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/EchoBird?style=flat-square&color=D97757" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/%E5%B9%B3%E5%8F%B0-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="平台" />
  <img src="https://img.shields.io/badge/%E6%8A%80%E6%9C%AF-Tauri%20%2B%20Rust-orange?style=flat-square" alt="Tauri + Rust" />
  <img src="https://img.shields.io/badge/%E8%AE%B8%E5%8F%AF-MIT-green?style=flat-square" alt="MIT 许可" />
</p>

<p align="center">
  <a href="https://echobird.ai">官网</a> ·
  <a href="https://github.com/edison7009/EchoBird/releases/latest">下载</a> ·
  <a href="README.md">English README</a>
</p>

---

## 这是什么

很多朋友让我帮他们安装 **Claude Code**、**OpenClaw**、**Hermes Agent**…… 每个人的系统都不一样,有些人还抠门到不愿花钱买大模型,安装和解释起来都特别费劲。

于是我做了 **EchoBird** —— 名字致敬《赛博朋克 2077》里的女网客 **Songbird(夜莺)**,她总能帮主角 V 搞定一切技术难题。EchoBird 也是这样:一键装好所有 AI 工具,把它们指向你选定的模型(云端 / 本地 / 中转),然后安静地退到一边。

## 亮点

- **一键安装** Claude Code、OpenClaw、Hermes Agent 等多款主流 AI 工具
- **自带模型** —— OpenAI / Anthropic / 本地大模型 / 中转站,统一管理
- **一键测速** —— 在使用前先看清每个模型的真实延迟
- **本地大模型运行时** —— 选好量化版本,按下 START 就能跑
- **安装与修复 Agent** —— 出问题时,像跟同事聊天一样把它搞定
- **跨平台** —— Windows、macOS、Linux(x64 + arm64)

## 界面截图

<table>
<tr>
  <td width="50%"><img src="docs/screenshots/news-cn.png" alt="AI 资讯" /></td>
  <td width="50%"><img src="docs/screenshots/model-cn.png" alt="模型中心" /></td>
</tr>
<tr>
  <td align="center"><sub>AI 资讯 & 明星项目</sub></td>
  <td align="center"><sub>模型中心 —— 管理与一键测速</sub></td>
</tr>
<tr>
  <td width="50%"><img src="docs/screenshots/app-cn.png" alt="应用管理" /></td>
  <td width="50%"><img src="docs/screenshots/localllm-cn.png" alt="本地大模型" /></td>
</tr>
<tr>
  <td align="center"><sub>应用管理 —— 一键启动</sub></td>
  <td align="center"><sub>本地大模型 —— 在自己机器上跑</sub></td>
</tr>
<tr>
  <td colspan="2"><img src="docs/screenshots/agent-cn.png" alt="安装与修复" /></td>
</tr>
<tr>
  <td colspan="2" align="center"><sub>安装与修复 —— 用对话搞定部署和排障</sub></td>
</tr>
</table>

## 安装

### 一行命令安装

**Windows**(PowerShell)

```powershell
irm https://echobird.ai/install.ps1 | iex
```

**macOS / Linux**

```sh
curl -fsSL https://echobird.ai/install.sh | sh
```

脚本会自动识别你的系统,下载对应的安装包,如果你已经是最新版会自动跳过。

### 或者下载安装包

最新版本 → <https://github.com/edison7009/EchoBird/releases/latest>

| 平台 | 安装包 |
|---|---|
| Windows x64 | `EchoBird_<ver>_Windows_x64-setup.exe` |
| macOS(Apple Silicon) | `EchoBird_<ver>_macOS_arm64.dmg` |
| Linux x64 · Debian/Ubuntu | `EchoBird_<ver>_Linux_x64.deb` |
| Linux arm64 · Debian/Ubuntu | `EchoBird_<ver>_Linux_arm64.deb` |
| Linux x64 · Fedora/RHEL | `EchoBird_<ver>_Linux_x64.rpm` |
| Linux arm64 · Fedora/RHEL | `EchoBird_<ver>_Linux_arm64.rpm` |

## 许可

MIT —— 详见 [LICENSE](LICENSE)。

---

<p align="center">
  Made with 💚 by EchoBird Team<br>
  <sub>⭐ <a href="https://github.com/edison7009/EchoBird">在 GitHub 上点个 Star</a> · <a href="README.md">English README</a></sub>
</p>
