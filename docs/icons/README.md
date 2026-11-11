# 图标目录说明

## 目录结构

```
public/icons/
├── models/     # AI 模型图标
└── tools/      # AI 编程工具图标
```

## 图标规范

### 工具图标 (tools/)
放置 AI 编程工具的图标文件：
- `openclawd.svg` - OpenClawd 图标
- `claude-code.svg` - Claude Code 图标
- `opencode.svg` - OpenCode 图标
- `codex.svg` - Codex 图标

### 模型图标 (models/)
放置 AI 模型的图标文件：
- `qwen3-48b.svg` - Qwen3-48b 模型图标
- `gemini-3-pro.svg` - Gemini-3-Pro 模型图标
- `gpt-4o.svg` - GPT-4o 模型图标
- 等等...

## 图标要求

- **格式**: SVG（推荐）或 PNG
- **尺寸**: 建议 64x64 或 128x128 像素
- **命名**: 全小写，使用连字符分隔
- **风格**: 建议统一风格，符合赛博朋克主题

## 使用方式

在代码中引用图标：
```tsx
icon: '/icons/tools/openclawd.svg'
icon: '/icons/models/qwen3-48b.svg'
```
