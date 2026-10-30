import { describe, it, expect } from 'vitest';
import { getModelIcon } from './ModelCard';

describe('getModelIcon', () => {
  it('maps the Tencent Hunyuan model id to the existing Hunyuan icon', () => {
    expect(getModelIcon('', 'hy')).toBe('./icons/models/hunyuan.svg');
  });

  it('uses the new Agnes SVG for directory rows and model cards', () => {
    expect(getModelIcon('Agnes AI', '')).toBe('./icons/models/agnes.svg');
    expect(getModelIcon('Agnes AI', 'agnes-2.5-flash')).toBe('./icons/models/agnes.svg');
  });
  it('maps Kilo provider rows and endpoints to the existing tool icon', () => {
    expect(getModelIcon('Kilo AI', '')).toBe('./icons/tools/kilo.png');
    expect(getModelIcon('', 'https://api.kilo.ai/api/gateway')).toBe('./icons/tools/kilo.png');
  });

  it('keeps the model brand winning over the Kilo gateway for real ids', () => {
    expect(getModelIcon('Kilo AI', 'anthropic/claude-sonnet-4.5')).toBe(
      './icons/models/claude.svg'
    );
    expect(getModelIcon('Kilo AI', 'deepseek/deepseek-v3.2')).toBe('./icons/models/deepseek.svg');
    expect(getModelIcon('Kilo AI', 'openai/gpt-5')).toBe('./icons/models/chatgpt.svg');
  });

  it('maps OpenCode Zen / Go provider rows to the opencode icon', () => {
    // Provider/directory rows pass modelId='' — the vendor name must win.
    expect(getModelIcon('OpenCode Zen', '')).toBe('./icons/models/opencode.svg');
    expect(getModelIcon('OpenCode Go', '')).toBe('./icons/models/opencode.svg');
  });

  it('keeps the model brand winning over the OpenCode gateway for real ids', () => {
    // A model configured through OpenCode Zen still shows its own brand logo,
    // because the id identifies the actual model (claude/gpt/deepseek), not
    // the gateway. The opencode rule sits below the model-brand rules.
    expect(getModelIcon('', 'claude-sonnet-5')).toBe('./icons/models/claude.svg');
    expect(getModelIcon('', 'deepseek-v4-pro')).toBe('./icons/models/deepseek.svg');
    expect(getModelIcon('', 'gpt-5.5')).toBe('./icons/models/chatgpt.svg');
  });

  it('maps free-router provider rows to their supplied icons', () => {
    expect(getModelIcon('Cerebras', '')).toBe('./icons/models/cerebras.svg');
    expect(getModelIcon('ModelScope', '')).toBe('./icons/models/modelscope.svg');
    expect(getModelIcon('Ollama Cloud', '')).toBe('./icons/models/ollama.svg');
    expect(getModelIcon('SambaNova Cloud', '')).toBe('./icons/models/sambanova.svg');
    expect(getModelIcon('Scaleway Generative APIs', '')).toBe('./icons/models/scaleway.svg');
    expect(getModelIcon('Typhoon (SCB 10X)', '')).toBe('./icons/models/typhoon.svg');
    expect(getModelIcon('W&B Inference', '')).toBe('./icons/models/wandb.svg');
  });
});
