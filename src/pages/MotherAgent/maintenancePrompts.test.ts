import { expect, it } from 'vitest';
import { maintenanceMessage } from './maintenancePrompts';

it.each([
  ['Analyze/clean leftovers after uninstall', 'en'],
  ['分析/清理无法被卸载的残留', 'zh-Hans'],
  ['分析/清理無法被解除安裝的殘留', 'zh-Hant'],
  ['アンインストール後の残留ファイルを分析・整理', 'ja'],
])('attaches analysis-first instructions to the explicit shortcut %s', (label, locale) => {
  const message = `${label}\nOnly inspect the D drive; keep all game saves.`;
  const result = maintenanceMessage(
    message,
    { residuals: label, diskSpace: '' },
    'remote-fixture',
    locale
  );
  expect(result.startsWith(message)).toBe(true);
  expect(result).toContain('Target machine: selected SSH server "remote-fixture"');
  expect(result).toContain(`Response language: ${locale}`);
  expect(result).toContain('## 1.');
  expect(result).toContain('## 2.');
  expect(result).toContain('## 3.');
  expect(result).toContain('Stop after analysis');
  expect(result).toContain('USER CONTENT');
  expect(result).toContain('Never execute a candidate script');
});
it('does not expand ordinary messages or quoted shortcut text', () => {
  for (const text of [
    'Clean only C:/Temp/example.log',
    'Explain this label: Analyze/clean leftovers after uninstall',
    '清理 1、3',
  ]) {
    expect(
      maintenanceMessage(
        text,
        {
          residuals: 'Analyze/clean leftovers after uninstall',
          diskSpace: 'Analyze disk space usage',
        },
        'local',
        'en'
      )
    ).toBe(text);
  }
});

it.each([
  ['Analyze disk space usage', 'en'],
  ['分析磁盘空间占用', 'zh-Hans'],
  ['分析磁碟空間占用', 'zh-Hant'],
  ['ディスク容量の使用状況を分析', 'ja'],
])('attaches the independent disk analysis workflow for %s', (label, locale) => {
  const message = `${label}\nOnly inspect D:/Projects; keep all model files.`;
  const result = maintenanceMessage(
    message,
    { residuals: 'Other cleanup', diskSpace: label },
    'local',
    locale
  );
  expect(result.startsWith(message)).toBe(true);
  expect(result).toContain('<echobird-disk-space>');
  expect(result).not.toContain('<echobird-residual-cleanup>');
  expect(result).toContain('READ-ONLY ANALYSIS');
  expect(result).toContain('allocated disk usage');
  expect(result).toContain('apparent/logical file size');
  expect(result).toContain('not automatically junk');
  expect(result).toContain('Target machine: local computer');
});

it.each([
  ['分析磁盘空间占用', 'en', 'echobird-disk-space'],
  ['Analyze/clean leftovers after uninstall', 'zh-Hans', 'echobird-residual-cleanup'],
])('retains the read-only workflow after the UI language changes (%s)', (draft, locale, tag) => {
  const result = maintenanceMessage(
    draft,
    { residuals: 'Different current label', diskSpace: 'Another current label' },
    'remote-scope',
    locale
  );
  expect(result.startsWith(draft)).toBe(true);
  expect(result).toContain(`<${tag}>`);
  expect(result).toContain('Target machine: selected SSH server "remote-scope"');
  expect(result).toContain(`Response language: ${locale}`);
});
