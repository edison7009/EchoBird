import fs from 'fs';
import path from 'path';

const docsDir = 'd:/Echobird/docs';

const translations = {
  'zh-CN': {
    lang: '简体中文',
    whatIs: '✨ Echobird 是什么？',
    desc: 'Echobird 是一款桌面应用，为你的 AI 编程工具提供**可视化、统一的模型管理界面**。无需再手动翻配置文件 —— 点一下，就能切换。',
    problem: '痛点',
    p1: '😫 在 OpenClaw 等工具中切换模型需要手动编辑配置文件',
    p2: '🔄 每个工具都有自己的模型配置格式',
    p3: '🧩 没有方便的方式管理技能和扩展',
    solution: '解决方案',
    solutionDesc: 'Echobird 是你所有 AI 编程工具的**中央控制面板**：',
    f1: '🎯 **一键切换模型** — 可视化切换任何支持工具的 AI 模型',
    f2: '🔀 **双协议支持** — OpenAI & Anthropic API 支持，随时随地切换模型',
    f3: '🚇 **智能隧道代理** — 无需全局 VPN 即可访问受限 API，仅代理 API 流量',
    f4: '🧩 **技能浏览器** — 发现、安装和管理 AI 技能',
    f5: '🖥️ **本地模型服务器** — 通过 llama.cpp 本地运行开源模型（Qwen、DeepSeek、Llama）',
    f6: '🌍 **28 种语言** — 完整国际化支持',
    f7: '🎮 **内置 AI 应用** — 交互式 AI 游戏和工具，如 Reversi 和 AI 翻译',
    f8: '🌃 **赛博朋克 UI** — 炫酷的霓虹绿终端美学，让编程充满未来感',
    screenshots: '🖼️ 截图',
    ss1: 'Model Nexus — 在一处管理所有 AI 模型',
    ss2: 'App Manager — 一键为所有编程工具切换模型',
    ss3: 'Local Server — 通过 llama.cpp 本地运行开源模型',
    ss4: 'Skill Browser — 发现和安装 AI 技能',
    quickStart: '🚀 快速开始',
    download: '下载',
    downloadDesc: '获取适合你平台的最新版本：',
    platform: '平台',
    linuxNotes: 'Linux 说明',
    fuseNote: '如果遇到 FUSE 错误：',
    supportedTools: '🔧 支持的工具',
    tool: '工具', status: '状态', modelSwitching: '模型切换', protocol: '协议',
    supported: '✅ 已支持',
    techStack: '🏗️ 技术栈',
    t1: '**Electron** — 跨平台桌面框架',
    t2: '**React + TypeScript** — UI 框架',
    t3: '**Vanilla CSS** — 自定义赛博朋克设计系统',
    t4: '**Vite** — 构建工具',
    t5: '**llama.cpp** — 本地模型推理引擎',
    dev: '🛠️ 开发',
    contributing: '🤝 贡献',
    contribDesc: '欢迎贡献！随时提交 Issue 或 Pull Request。',
    contribHelp: 'We\'re especially looking for help with:\n- 🍎 **macOS 测试** — 我们还没有完全测试 macOS 构建\n- 🔧 **新工具集成** — 帮助我们支持更多 AI 编程工具\n- 🌐 **翻译改进** — 欢迎母语使用者！',
    support: '⭐ 支持',
    supportDesc: '如果 Echobird 对你有帮助，请在 GitHub 上给个 ⭐ —— 让更多人发现这个项目！',
    license: '📄 许可证',
    madeWith: '由 Echobird 团队用 💚 打造'
  },
  'zh-TW': {
    lang: '繁體中文',
    whatIs: '✨ Echobird 是什麼？',
    desc: 'Echobird 是一款桌面應用，為你的 AI 程式設計工具提供**視覺化、統一的模型管理介面**。不再需要手動翻設定檔 —— 點一下，就能切換。',
    problem: '痛點',
    p1: '😫 在 OpenClaw 等工具中切換模型需要手動編輯設定檔',
    p2: '🔄 每個工具都有自己的模型設定格式',
    p3: '🧩 沒有方便的方式管理技能和擴充',
    solution: '解決方案',
    solutionDesc: 'Echobird 是你所有 AI 程式設計工具的**中央控制面板**：',
    f1: '🎯 **一鍵切換模型** — 視覺化切換任何支援工具的 AI 模型',
    f2: '🔀 **雙協議支援** — OpenAI & Anthropic API 支援，隨時隨地切換模型',
    f3: '🚇 **智慧隧道代理** — 無需全域 VPN 即可存取受限 API，僅代理 API 流量',
    f4: '🧩 **技能瀏覽器** — 發現、安裝和管理 AI 技能',
    f5: '🖥️ **本地模型伺服器** — 透過 llama.cpp 本地執行開源模型（Qwen、DeepSeek、Llama）',
    f6: '🌍 **28 種語言** — 完整國際化支援',
    f7: '🎮 **內建 AI 應用** — 互動式 AI 遊戲和工具，如 Reversi 和 AI 翻譯',
    f8: '🌃 **賽博龐克 UI** — 炫酷的霓虹綠終端美學，讓程式設計充滿未來感',
    screenshots: '🖼️ 截圖',
    ss1: 'Model Nexus — 在一處管理所有 AI 模型',
    ss2: 'App Manager — 一鍵為所有程式設計工具切換模型',
    ss3: 'Local Server — 透過 llama.cpp 本地執行開源模型',
    ss4: 'Skill Browser — 發現和安裝 AI 技能',
    quickStart: '🚀 快速開始',
    download: '下載',
    downloadDesc: '取得適合你平台的最新版本：',
    platform: '平台',
    linuxNotes: 'Linux 說明',
    fuseNote: '如果遇到 FUSE 錯誤：',
    supportedTools: '🔧 支援的工具',
    tool: '工具', status: '狀態', modelSwitching: '模型切換', protocol: '協議',
    supported: '✅ 已支援',
    techStack: '🏗️ 技術棧',
    t1: '**Electron** — 跨平台桌面框架',
    t2: '**React + TypeScript** — UI 框架',
    t3: '**Vanilla CSS** — 自訂賽博龐克設計系統',
    t4: '**Vite** — 建置工具',
    t5: '**llama.cpp** — 本地模型推理引擎',
    dev: '🛠️ 開發',
    contributing: '🤝 貢獻',
    contribDesc: '歡迎貢獻！隨時提交 Issue 或 Pull Request。',
    contribHelp: 'We\'re especially looking for help with:\n- 🍎 **macOS 測試** — 我們還沒有完全測試 macOS 構建',
    support: '⭐ 支持',
    supportDesc: '如果 Echobird 對你有幫助，請在 GitHub 上給個 ⭐ —— 讓更多人發現這個專案！',
    license: '📄 授權條款',
    madeWith: '由 Echobird 團隊用 💚 打造'
  },
  'ja': {
    lang: '日本語',
    whatIs: '✨ Echobird とは？',
    desc: 'Echobird は、AIコーディングツール全体でモデルを管理するための**ビジュアルで統一されたインターフェース**を提供するデスクトップアプリです。',
    problem: '課題',
    p1: '😫 OpenClaw などのツールでAIモデルを切り替えるには設定ファイルの手順が必要',
    p2: '🔄 各ツールが独自のモデル設定形式を持っている',
    p3: '🧩 ツール間でスキルや拡張機能を管理する方法がない',
    solution: 'ソリューション',
    solutionDesc: 'Echobird はすべてのAIコーディングツールの**中央コントロールパネル**として機能します：',
    f1: '🎯 **ワンクリックモデル切替** — 対応ツールのAIモデルをビジュアルに切り替え',
    f2: '🔀 **デュアルプロトコル** — OpenAI & Anthropic API対応',
    f3: '🚇 **スマートトンネルプロキシ** — フルVPNなしで地域制限APIにアクセス',
    f4: '🧩 **スキルブラウザ** — AIスキルを発見、インストール、管理',
    f5: '🖥️ **ローカル模型サーバー** — llama.cpp経由でオープンソースモデルをローカル実行',
    f6: '🌍 **28言語対応** — グローバル対応の完全国際化',
    f7: '🎮 **内蔵AIアプリ** — Reversi やAI翻訳などのインタラクティブなAIゲーム',
    f8: '🌃 **サイバーパンク UI** — ネオングリーンのターミナル美学',
    screenshots: '🖼️ スクリーンショット',
    ss1: 'Model Nexus — すべてのAIモデルを一箇所で管理',
    ss2: 'App Manager — ワンクリックでモデル切替',
    ss3: 'Local Server — オープンソースモデルをローカル実行',
    ss4: 'Skill Browser — スキルを発見・インストール',
    quickStart: '🚀 クイックスタート',
    download: 'ダウンロード',
    downloadDesc: '最新リリースを入手：',
    platform: 'プラットフォーム',
    linuxNotes: 'Linux の注意事項',
    fuseNote: 'FUSE エラーが発生した場合：',
    supportedTools: '🔧 対応ツール',
    tool: 'ツール', status: 'ステータス', modelSwitching: 'モデル切替', protocol: 'プロトコル',
    supported: '✅ 対応済み',
    techStack: '🏗️ 技術スタック',
    t1: '**Electron** — デスクトップフレームワーク',
    t2: '**React + TypeScript** — UIフレームワーク',
    t3: '**Vanilla CSS** — デザインシステム',
    t5: '**llama.cpp** — 推論エンジン',
    dev: '🛠️ 開発',
    contributing: '🤝 コントリビュート',
    contribDesc: 'コントリビュート大歓迎！',
    support: '⭐ サポート',
    supportDesc: 'Echobird が役立ったら、GitHub で ⭐ をお願いします！',
    license: '📄 ライセンス',
    madeWith: 'Echobird チームが 💚 を込めて制作'
  },
  'ko': {
    lang: '한국어',
    whatIs: '✨ Echobird란?',
    desc: 'Echobird는 AI 코딩 도구 전반에 걸쳐 모델을 관리하기 위한 **시각적이고 통합된 인터페이스**를 제공하는 애플리케이션입니다.',
    problem: '문제점',
    p1: '😫 OpenClaw 같은 도구에서 AI 모델을 전환하려면 설정 파일을 수동으로 편집해야 함',
    p2: '🔄 각 도구마다 고유한 구성 형식이 있음',
    p3: '🧩 스킬과 확장 기능을 관리할 편리한 방법이 없음',
    solution: '솔루션',
    solutionDesc: 'Echobird는 모든 AI 코딩 도구의 **중앙 제어 패널** 역할을 합니다:',
    f1: '🎯 **원클릭 모델 전환** — AI 모델을 시각적으로 전환',
    f2: '🔀 **듀얼 프로토콜** — OpenAI & Anthropic API 지원',
    f3: '🚇 **스마트 터널 프록시** — VPN 없이 지역 제한 API에 접근',
    f4: '🧩 **스킬 브라우저** — AI 스킬 검색, 설치, 관리',
    f5: '🖥️ **로컬 모델 서버** — llama.cpp로 오픈소스 모델 로컬 실행',
    f6: '🌍 **28개 언어** — 글로벌 대응 완전 국제화',
    f7: '🎮 **내장 AI 앱** — Reversi, AI 번역 등',
    f8: '🌃 **사이버펑크 UI** — 미래지향적 코딩 경험',
    screenshots: '🖼️ 스크린샷',
    ss1: 'Model Nexus — 한 곳에서 모든 모델 관리',
    ss2: 'App Manager — 원클릭 모델 전환',
    ss3: 'Local Server — 로컬 실행',
    ss4: 'Skill Browser — 검색 및 설치',
    quickStart: '🚀 빠른 시작',
    download: '다운로드',
    downloadDesc: '최신 릴리스를 받으세요:',
    platform: '플랫폼',
    linuxNotes: 'Linux 참고사항',
    fuseNote: 'FUSE 오류가 발생하면：',
    supportedTools: '🔧 지원 도구',
    tool: '도구', status: '상태', modelSwitching: '모델 전환', protocol: '프로토콜',
    supported: '✅ 지원',
    techStack: '🏗️ 기술 스택',
    t1: '**Electron** — 프레임워크',
    t2: '**React + TypeScript** — UI',
    t3: '**Vanilla CSS** — 디자인',
    t5: '**llama.cpp** — 추론 엔진',
    dev: '🛠️ 개발',
    contributing: '🤝 기여',
    contribDesc: '기여를 환영합니다!',
    support: '⭐ 지원',
    supportDesc: 'Echobird가 유용하다면, GitHub에서 ⭐를 눌러주세요!',
    license: '📄 라이선스',
    madeWith: 'Echobird 팀이 💚으로 제작'
  }
};

// Simplified README template generator
function generateReadme(lang, t) {
  return `<p align="center">
  <img src="../build/icon.png" alt="Echobird" width="120" />
</p>

<h1 align="center">Echobird</h1>

<p align="center">
  The Nexus for <strong>Models</strong>, <strong>Agents</strong> & <strong>Vibe Coding</strong>.<br/>
  <sub>${t.desc.replace('**', '').replace('**', '')}</sub>
</p>

<p align="center">
  <a href="https://github.com/edison7009/Echobird/releases">
    <img src="https://img.shields.io/github/v/release/edison7009/Echobird?style=flat-square&color=00FF9D" alt="Release" />
  </a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue?style=flat-square" alt="Platform" />
  <img src="https://img.shields.io/github/license/edison7009/Echobird?style=flat-square" alt="License" />
</p>

<p align="center">
  <a href="../README.md">English</a> · ${Object.keys(translations).map(k => k === lang ? `**${translations[k].lang}**` : `<a href="./README.${k}.md">${translations[k].lang}</a>`).join(' · ')}
</p>

---

## ${t.whatIs}

${t.desc}

### ${t.problem}

- ${t.p1}
- ${t.p2}
- ${t.p3}

### ${t.solution}

${t.solutionDesc}

- ${t.f1}
- ${t.f2}
- ${t.f3}
- ${t.f4}
- ${t.f5}
- ${t.f6}
- ${t.f7}
- ${t.f8}

## ${t.screenshots}

### ${t.ss1}
![Model Nexus](./1.png)

### ${t.ss2}
![App Manager](./2.png)

### ${t.ss3}
![Local Server](./3.png)

### ${t.ss4}
![Skill Browser](./4.png)

## ${t.quickStart}

### ${t.download}

${t.downloadDesc}

| ${t.platform} | ${t.download} |
|----------|----------|
| Windows  | [Echobird-Setup.exe](https://github.com/edison7009/Echobird/releases/latest) |
| macOS    | [Echobird.dmg](https://github.com/edison7009/Echobird/releases/latest) |
| Linux    | [Echobird.AppImage](https://github.com/edison7009/Echobird/releases/latest) |

### ${t.linuxNotes}

\`\`\`bash
chmod +x Echobird-*.AppImage
./Echobird-*.AppImage
\`\`\`

> ${t.fuseNote} \`sudo apt install libfuse2\`

## ${t.supportedTools}

| ${t.tool} | ${t.status} | ${t.modelSwitching} | ${t.protocol} |
|------|--------|----------------|----------|
| OpenClaw | ✅ ${t.supported.replace('✅ ', '')} | ✅ | OpenAI / Anthropic |
| Claude Code | ✅ ${t.supported.replace('✅ ', '')} | ✅ | Anthropic |
| Cline | ✅ ${t.supported.replace('✅ ', '')} | ✅ | OpenAI |
| Continue | ✅ ${t.supported.replace('✅ ', '')} | ✅ | OpenAI |
| OpenCode | ✅ ${t.supported.replace('✅ ', '')} | ✅ | OpenAI |
| Codex | ✅ ${t.supported.replace('✅ ', '')} | ✅ | OpenAI |
| Roo Code | ✅ ${t.supported.replace('✅ ', '')} | ✅ | OpenAI |

## ${t.techStack}

- ${t.t1}
- ${t.t2}
- ${t.t3}
- **Vite** — 构建工具
- ${t.t5}

## ${t.dev}

\`\`\`bash
npm install
npm run dev
npm run build
\`\`\`

## ${t.contributing}

${t.contribDesc}

We're especially looking for help with:
- 🍎 **macOS 测试**
- 🔧 **新工具集成**
- 🌐 **翻译改进**

## ${t.support}

${t.supportDesc}

## ${t.license}

[MIT](../LICENSE)

---

<p align="center">
  ${t.madeWith}<br/>
  <sub>📧 <a href="mailto:hi@echobird.ai">hi@echobird.ai</a></sub>
</p>
`;
}

// Write README files
Object.keys(translations).forEach(lang => {
  const content = generateReadme(lang, translations[lang]);
  fs.writeFileSync(path.join(docsDir, `README.${lang}.md`), content, 'utf8');
});

console.log('Documentation updated successfully.');
