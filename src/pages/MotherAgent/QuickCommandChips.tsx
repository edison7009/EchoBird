import { useEffect, useState } from 'react';
import { useI18n } from '../../hooks/useI18n';
import * as api from '../../api/tauri';
import type { TKey } from '../../i18n';
import { useMotherAgent } from './context';
import { FRONTEND_HINTS } from './frontendHints';
import type { MotherHint } from './types';
import { useToolsStore } from '../../stores/toolsStore';
import type { LocalTool } from '../../api/types';

const pickToolName = (tool: LocalTool, locale: string): string => {
  if (locale === 'en' || !tool.names) return tool.name;
  const direct = tool.names[locale];
  if (direct) return direct;
  const base = locale.split('-')[0];
  if (tool.names[base]) return tool.names[base];
  const fuzzy = Object.entries(tool.names).find(([k]) => k.startsWith(base));
  return fuzzy?.[1] || tool.name;
};

// Quick-command chips for the Mother Agent welcome screen. Each chip drops a
// self-contained prompt into the chat input (the user edits, then sends).
// Hints come from the bundled registry (getMotherHints) with EchoBird's own
// FRONTEND_HINTS prepended; a hint with no localized label renders nothing,
// which is how locale-specific chips hide themselves in other UI languages.
export function QuickCommandChips() {
  const { t, locale } = useI18n();
  const { setChatInput, selectedServerId } = useMotherAgent();
  const detectedTools = useToolsStore((s) => s.detectedTools);
  const [hints, setHints] = useState<MotherHint[]>([]);

  useEffect(() => {
    api
      .getMotherHints()
      .then((s) => {
        try {
          const data = JSON.parse(s);
          setHints([...FRONTEND_HINTS, ...(data.hints || [])]);
        } catch {
          setHints([...FRONTEND_HINTS]);
        }
      })
      .catch(() => setHints([...FRONTEND_HINTS]));
  }, []);

  const renderCommand = (hint: MotherHint, i: number) => {
    // showSpecs swaps to a local-machine wording when 127.0.0.1 is
    // selected — otherwise the agent often refuses, treating "server"
    // prompts as a remote/privileged operation.
    const isLocalShowSpecs = hint.action === 'showSpecs' && selectedServerId === 'local';
    const i18nKey = (isLocalShowSpecs
      ? 'mother.hintShowSpecsLocal'
      : `mother.hint${hint.action[0].toUpperCase()}${hint.action.slice(1)}`) as unknown as TKey;
    // Localize the {agent} tool name (e.g. "Hermes 桌面端" in zh-Hans) using
    // the same names map the tool cards use, so install chips match the
    // cards instead of showing the raw English name. Falls back to the raw
    // name when the tool isn't in the scanned registry yet.
    const agentRaw = hint.agent || '';
    const matched = agentRaw ? detectedTools.find((tool) => tool.name === agentRaw) : undefined;
    const agentLabel = matched ? pickToolName(matched, locale) : agentRaw;
    const label = t(i18nKey).replace('{agent}', agentLabel);
    // Skip hints with no translation (label falls back to the raw key).
    if (label === i18nKey) return null;
    return (
      <button
        key={i}
        // The chat textarea lives in MotherAgentMain with a local ref we
        // can't reach from here; setting the shared input value is enough —
        // the text appears and the user takes it from there.
        onClick={() => setChatInput(label)}
        className="min-w-0 grow px-3 py-1.5 text-xs rounded-lg bg-cyber-surface/60 text-cyber-text-secondary hover:bg-cyber-elevated/70 hover:text-cyber-text transition-colors"
      >
        {label}
      </button>
    );
  };
  const maintenanceActions = ['analyzeResiduals', 'analyzeDiskSpace'];
  const commands = hints
    .filter((hint) => !maintenanceActions.includes(hint.action))
    .map(renderCommand)
    .filter((command) => command !== null);
  const maintenanceCommands = maintenanceActions.flatMap((action, index) => {
    const hint = hints.find((item) => item.action === action);
    const command = hint ? renderCommand(hint, -index - 1) : null;
    return command ? [command] : [];
  });
  const firstRowEnd = Math.ceil(commands.length * 0.4);
  const secondRowEnd = firstRowEnd + Math.ceil((commands.length - firstRowEnd) / 2);
  const rows = [
    commands.slice(0, firstRowEnd),
    commands.slice(firstRowEnd, secondRowEnd),
    commands.slice(secondRowEnd),
  ].filter((row) => row.length);

  return (
    <div className="mx-auto flex w-full max-w-[720px] flex-col items-center gap-2">
      {rows.map((row, index) => (
        <div key={index} className={`flex gap-2 ${['w-full', 'w-[88%]', 'w-[72%]'][index]}`}>
          {row}
        </div>
      ))}
      {maintenanceCommands.length > 0 && (
        <div className="flex max-w-full gap-2" data-command-row="4">
          {maintenanceCommands}
        </div>
      )}
    </div>
  );
}
