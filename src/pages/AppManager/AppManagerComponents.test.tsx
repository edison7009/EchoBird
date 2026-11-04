import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import {
  act,
  create,
  type ReactTestRenderer,
  type ReactTestRendererJSON,
} from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as api from '../../api/tauri';
import type { LocalTool, ModelConfig } from '../../api/types';
import type { TKey } from '../../i18n';
import { ModelListCard } from '../../components/ModelListCard';
import { SMART_ROUTER_SWITCH_TOOLS } from '../../utils/smartRouter';
import vscodeDefinition from '../../../tools/vscode/paths.json';
import qoderDefinition from '../../../tools/qoder/paths.json';
import qoderCnDefinition from '../../../tools/qodercn/paths.json';
import claudeScienceDefinition from '../../../tools/claudescience/paths.json';
import openDesignDefinition from '../../../tools/opendesign/paths.json';
import { useNavigationStore } from '../../stores/navigationStore';
import { AppManagerContext, type AppManagerContextType } from './context';
import { AccountSectionRow } from './AccountSectionPrimitives';

vi.mock('../../components', () => ({
  getModelIcon: () => null,
}));

vi.mock('../../utils/platform', () => ({ IS_WINDOWS: false, IS_LINUX: true, IS_MACOS: false }));

const tool: LocalTool = {
  id: 'test-tool',
  name: 'Test Tool',
  category: 'Desktop',
  installed: true,
  apiProtocol: ['openai'],
};

const models: ModelConfig[] = [
  {
    internalId: 'cloud-model',
    name: 'Cloud Model',
    modelId: 'deepseek-flash',
    baseUrl: 'https://cloud.example/v1',
    apiKey: '',
  },
  {
    internalId: 'local-server',
    name: 'Local Model',
    modelId: 'local-model-id',
    baseUrl: 'http://127.0.0.1:1234/v1',
    apiKey: '',
  },
  {
    internalId: 'smart-router',
    name: 'Auto Router',
    baseUrl: 'http://127.0.0.1:53683/v1',
    apiKey: '',
  },
];

const labels: Partial<Record<TKey, string>> = {
  'agent.badge.smart': '智能',
  'agent.badge.switch': '可切换',
  'agent.badge.local': '本地',
};

it('Open Design offers both API protocols and its native official mode in the shared model list', async () => {
  vi.stubGlobal('__APP_EDITION__', 'full');
  const { ModelListSection } = await import('./AppManagerComponents');
  expect(openDesignDefinition.noModelConfig).toBe(false);
  const markup = renderToStaticMarkup(
    <ModelListSection
      selectedToolData={{
        ...tool,
        ...openDesignDefinition,
        id: 'opendesign',
        category: 'Creative',
      }}
      userModels={[
        models[0],
        {
          ...models[0],
          internalId: 'anthropic-model',
          name: 'Anthropic API',
          baseUrl: '',
          anthropicUrl: 'https://fixture.test/anthropic',
        },
      ]}
      toolModelConfig={{}}
      selectedTool="opendesign"
      handleSelectModel={vi.fn()}
      t={(key) => key}
    />
  );
  expect(markup).toContain('Cloud Model');
  expect(markup).toContain('Anthropic API');
  expect(markup).toContain('Open Design Official');
});

it('CodeArts shows API models without an Official restore card', async () => {
  vi.stubGlobal('__APP_EDITION__', 'full');
  const { ModelListSection } = await import('./AppManagerComponents');
  const markup = renderToStaticMarkup(
    <ModelListSection
      selectedToolData={{ ...tool, id: 'codearts', name: 'CodeArts' }}
      userModels={[models[0]]}
      toolModelConfig={{}}
      selectedTool="codearts"
      handleSelectModel={vi.fn()}
      t={(key) => key}
    />
  );
  expect(markup).toContain('Cloud Model');
  expect(markup).not.toContain('CodeArts Official');
});

describe('ModelListSection', () => {
  it.each([
    ['qoder', qoderDefinition],
    ['qodercn', qoderCnDefinition],
  ])('shows configured API models for %s', async (toolId, definition) => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    const { ModelListSection } = await import('./AppManagerComponents');
    expect(definition.noModelConfig).toBe(false);
    const markup = renderToStaticMarkup(
      <ModelListSection
        selectedToolData={{ ...tool, ...definition, id: toolId }}
        userModels={models}
        toolModelConfig={{}}
        selectedTool={toolId}
        handleSelectModel={vi.fn()}
        t={(key) => key}
      />
    );
    expect(markup).toContain('Cloud Model');
    expect(markup).toContain('deepseek-flash');
  });
  it('shows Anthropic models and the official login for Claude Science', async () => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    const { ModelListSection } = await import('./AppManagerComponents');
    expect(claudeScienceDefinition.noModelConfig).toBe(false);
    const markup = renderToStaticMarkup(
      <ModelListSection
        selectedToolData={{ ...tool, ...claudeScienceDefinition, id: 'claudescience' }}
        userModels={[{ ...models[0], anthropicUrl: 'https://cloud.example/anthropic' }]}
        toolModelConfig={{}}
        selectedTool="claudescience"
        handleSelectModel={vi.fn()}
        t={(key) => key}
      />
    );
    expect(markup).toContain('Cloud Model');
    expect(markup).toContain('Anthropic Official');
  });
  it.each(['test-tool', 'codex', 'chatgptdesktop'])(
    'hides the router for %s when smart routing is turned off',
    async (toolId) => {
      vi.stubGlobal('__APP_EDITION__', 'full');
      const { ModelListSection } = await import('./AppManagerComponents');
      const markup = renderToStaticMarkup(
        <ModelListSection
          smartRouterEnabled={false}
          selectedToolData={{ ...tool, id: toolId }}
          userModels={models}
          toolModelConfig={{ [toolId]: 'smart-router' }}
          selectedTool={toolId}
          handleSelectModel={vi.fn()}
          t={(key) => key}
        />
      );
      expect(markup).not.toContain('Auto Router');
      expect(markup).not.toContain('127.0.0.1:53683');
      expect(markup).toContain('Cloud Model');
    }
  );
  it('renders smart, local, and cloud models as one ordered list with compact badges', async () => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    const { ModelListSection } = await import('./AppManagerComponents');
    const markup = renderToStaticMarkup(
      <ModelListSection
        selectedToolData={tool}
        userModels={models}
        toolModelConfig={{}}
        selectedTool={tool.id}
        handleSelectModel={() => undefined}
        t={(key) => labels[key] ?? key}
      />
    );

    expect(markup).toContain('智能');
    expect(markup).toContain('本地');
    expect(markup.indexOf('Auto Router')).toBeLessThan(markup.indexOf('Local Model'));
    expect(markup.indexOf('Local Model')).toBeLessThan(markup.indexOf('Cloud Model'));
  });

  it('shows the model ID and usage instead of the API URL without extra tooltips', async () => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    const { ModelListSection } = await import('./AppManagerComponents');
    const markup = renderToStaticMarkup(
      <ModelListSection
        selectedToolData={{ ...tool, apiProtocol: ['openai', 'anthropic'] }}
        userModels={[{ ...models[0], anthropicUrl: 'https://cloud.example/anthropic' }]}
        toolModelConfig={{}}
        selectedTool={tool.id}
        handleSelectModel={() => undefined}
        modelUsageData={{
          'cloud-model': {
            quotas: [{ percentage: 80, resetAt: Date.now() + 60_000 }],
          },
        }}
        refreshingUsageIds={new Set()}
        onRefreshUsage={() => undefined}
        onEditModel={() => undefined}
        onDeleteModel={() => undefined}
        t={(key) => labels[key] ?? key}
      />
    );

    expect(markup).toContain('80%');
    expect(markup).toContain('deepseek-flash');
    expect(markup).not.toContain('cloud.example');
    expect(markup).not.toContain('OAI');
    expect(markup).not.toContain('ANT');
    expect(markup).not.toContain('⇄');
    expect(markup).not.toContain('OpenAI');
    expect(markup).not.toContain('Anthropic');
    expect(markup).not.toContain('title=');
    expect(markup).not.toContain('disabled=');
  });

  it('keeps router and official addresses while local and cloud models show IDs', async () => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    const { ModelListSection } = await import('./AppManagerComponents');
    const markup = renderToStaticMarkup(
      <ModelListSection
        selectedToolData={{ ...tool, id: 'claudedesktop' }}
        userModels={models}
        toolModelConfig={{}}
        selectedTool="claudedesktop"
        handleSelectModel={vi.fn()}
        t={(key) => key}
      />
    );
    expect(markup).toContain('127.0.0.1:53683/v1');
    expect(markup).toContain('api.anthropic.com');
    expect(markup).toContain('local-model-id');
    expect(markup).toContain('deepseek-flash');
    expect(markup).not.toContain('127.0.0.1:1234');
    expect(markup).not.toContain('cloud.example');
  });

  it.each(['codex', 'chatgptdesktop'])(
    'shows the Responses-capable router and hides the Chat-only local server from %s',
    async (toolId) => {
      vi.stubGlobal('__APP_EDITION__', 'full');
      const { ModelListSection } = await import('./AppManagerComponents');
      const markup = renderToStaticMarkup(
        <ModelListSection
          selectedToolData={{ ...tool, id: toolId }}
          userModels={models}
          toolModelConfig={{}}
          selectedTool={toolId}
          handleSelectModel={() => undefined}
          t={(key) => labels[key] ?? key}
        />
      );

      expect(markup).toContain('Auto Router');
      expect(markup).toContain('127.0.0.1:53683/v1');
      expect(markup).not.toContain('Local Model');
      expect(markup).not.toContain('OpenAI Official');
      expect(markup).toContain('Cloud Model');
    }
  );

  it.each([...SMART_ROUTER_SWITCH_TOOLS])(
    'selects the exact router card for %s without applying configuration',
    async (toolId) => {
      vi.stubGlobal('__APP_EDITION__', 'full');
      const { ModelListSection } = await import('./AppManagerComponents');
      const selectModel = vi.fn();
      const applyModel = vi.spyOn(api, 'applyModelToTool');
      const router = { ...models[2], modelId: 'auto' };
      let renderer: ReactTestRenderer | undefined;
      try {
        act(() => {
          renderer = create(
            <ModelListSection
              selectedToolData={{ ...tool, id: toolId }}
              userModels={[router]}
              toolModelConfig={{}}
              selectedTool={toolId}
              handleSelectModel={selectModel}
              t={(key) => key}
            />
          );
        });
        expect(selectModel).not.toHaveBeenCalled();
        expect(applyModel).not.toHaveBeenCalled();
        const card = renderer!.root.findByType(ModelListCard);
        expect(card.props.model).toEqual(router);
        expect(card.props.subtitle).toBe('127.0.0.1:53683/v1');
        const badges = renderToStaticMarkup([card.props.badge, card.props.secondaryBadge]);
        expect(badges).toContain('agent.badge.smart');
        expect(badges).toContain('agent.badge.switch');
        act(() => card.findByType('button').props.onClick());
        expect(selectModel).toHaveBeenCalledExactlyOnceWith(toolId, 'smart-router');
        expect(applyModel).not.toHaveBeenCalled();
      } finally {
        act(() => renderer?.unmount());
        applyModel.mockRestore();
      }
    }
  );

  it.each(['codex', 'claudecode', 'grok', 'test-tool'])(
    'only labels Smart Access cards with implemented switching for %s',
    async (toolId) => {
      vi.stubGlobal('__APP_EDITION__', 'full');
      const { ModelListSection } = await import('./AppManagerComponents');
      let renderer: ReactTestRenderer | undefined;
      try {
        act(() => {
          renderer = create(
            <ModelListSection
              selectedToolData={{ ...tool, id: toolId }}
              userModels={models}
              toolModelConfig={{ [toolId]: 'smart-router' }}
              selectedTool={toolId}
              handleSelectModel={vi.fn()}
              t={(key) => labels[key] ?? key}
            />
          );
        });
        for (const card of renderer!.root.findAllByType(ModelListCard)) {
          const badge = renderToStaticMarkup([
            card.props.badge ?? null,
            card.props.secondaryBadge ?? null,
          ]);
          expect(badge).not.toContain('title=');
          const supportsSwitch =
            card.props.model.internalId === 'smart-router' && SMART_ROUTER_SWITCH_TOOLS.has(toolId);
          expect(badge.includes('可切换')).toBe(supportsSwitch);
          if (card.props.model.internalId === 'smart-router')
            expect(card.props.selected).toBe(true);
        }
      } finally {
        act(() => renderer?.unmount());
      }
    }
  );

  it('hides the Grok official card while keeping user models', async () => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    const { ModelListSection } = await import('./AppManagerComponents');
    const markup = renderToStaticMarkup(
      <ModelListSection
        selectedToolData={{ ...tool, id: 'grok' }}
        userModels={models}
        toolModelConfig={{}}
        selectedTool="grok"
        handleSelectModel={vi.fn()}
        t={(key) => key}
      />
    );

    expect(markup).not.toContain('xAI Official');
    expect(markup).not.toContain('api.x.ai');
    expect(markup).toContain('Cloud Model');
    expect(markup).toContain('Local Model');
  });
});

describe('VS Code model panel', () => {
  it.each([false, true])('shows compatible models only when installed: %s', async (installed) => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    const { AppManagerPanel, AppManagerBottom } = await import('./AppManagerComponents');
    const { ModelNexusContext } = await import('../ModelNexus/context');
    const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
    useNavigationStore.setState({ activePage: 'apps' });
    const context = {
      selectedTool: 'vscode',
      selectedToolData: { ...vscodeDefinition, id: 'vscode', installed },
      smartRouterEnabled: true,
      userModels: [
        ...models,
        {
          internalId: 'anthropic-model',
          name: 'Anthropic Model',
          modelId: 'anthropic-model-id',
          baseUrl: '',
          anthropicUrl: 'https://example.test/anthropic',
          apiKey: '',
        },
      ],
      toolModelConfig: { vscode: 'cloud-model' },
      handleSelectModel: vi.fn(),
      viewMode: 'desktop',
      launchAfterApply: false,
      agreedConfigPolicy: true,
      isLaunching: false,
    } as unknown as AppManagerContextType;
    const markup = renderToStaticMarkup(
      <ConfirmDialogProvider>
        <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
          <AppManagerContext.Provider value={context}>
            <AppManagerPanel />
            <AppManagerBottom />
          </AppManagerContext.Provider>
        </ModelNexusContext.Provider>
      </ConfirmDialogProvider>
    );
    for (const name of ['Cloud Model', 'Local Model', 'Auto Router', 'Anthropic Model']) {
      expect(markup.includes(name)).toBe(installed);
    }
    expect(markup).not.toContain('agent.noModelConfig');
    expect(markup).not.toContain('agent.addAccount');
    expect(markup).not.toContain('role="switch"');
    expect(markup).not.toContain('agent.restore');
    expect(markup).toContain(installed ? 'btn.modifyOnly' : 'btn.installOneClick');
    expect(context.handleSelectModel).not.toHaveBeenCalled();
  });
});

describe('account launch options', () => {
  const accountTools = [
    ['chatgptdesktop', 'selectedCodexAccountId'],
    ['codex', 'selectedCodexAccountId'],
    ['antigravity', 'antigravityAccounts'],
    ['antigravitydesktop', 'antigravityAccounts'],
    ['codearts', 'codeArtsAccounts'],
    ['minimaxcode', 'minimaxAccounts'],
    ['minimaxdesktop', 'minimaxAccounts'],
    ['qoder', 'qoderAccounts'],
    ['qodercn', 'qoderAccounts'],
    ['dumate', 'dumateAccounts'],
    ['doubaowork', 'doubaoAccounts'],
    ['dola', 'doubaoAccounts'],
    ['qwenwork', 'qwenWorkAccounts'],
    ['qwenworkcn', 'qwenWorkAccounts'],
    ['zcode', 'zcodeAccounts'],
    ['kimicode', 'kimiAccounts'],
    ['kimidesktop', 'kimiAccounts'],
    ['devin', 'devinAccounts'],
    ['devincli', 'devinAccounts'],
    ['opendesign', 'openDesignAccounts'],
    ['claudecode', 'claudeCodeAccounts'],
    ['claudedesktop', 'claudeDesktopAccounts'],
    ['dsh', 'deepSeekAccounts'],
    ['grok', 'grokAccounts'],
    ['cline', 'clineAccounts'],
    ['clinedesktop', 'clineAccounts'],
    ['kilo', 'kiloAccounts'],
    ['kilodesktop', 'kiloAccounts'],
    ['copilot', 'copilotAccounts'],
    ['copilotdesktop', 'copilotAccounts'],
    ['manus', 'manusAccounts'],
    ['cue', 'cueAccounts'],
    ['grokbot', 'grokBotAccounts'],
    ['cursor', 'cursorAccounts'],
    ['cursorcli', 'cursorAccounts'],
    ['workbuddy', 'workBuddyAccounts'],
    ['workbuddyai', 'workBuddyAccounts'],
    ['trae', 'traeAccounts'],
    ['traecn', 'traeAccounts'],
    ['kirocrew', 'kiroAccounts'],
    ['kirocli', 'kiroAccounts'],
  ] as const;

  it.each([
    ['an installed CLI', { category: 'CLI Code', installed: true }, true],
    ['an uninstalled CLI', { category: 'CLI Code', installed: false }, false],
    ['a desktop app', { category: 'Desktop', installed: true }, false],
    ['a plugin CLI', { category: 'CLI Code', installed: true, plugin: {} }, false],
  ])('shows launch settings beside the launch button for %s only', async (_, data, shown) => {
    const { AppManagerBottom } = await import('./AppManagerComponents');
    const context = {
      selectedTool: 'fixture',
      selectedToolData: { ...tool, id: 'fixture', ...data },
      toolModelConfig: {},
      viewMode: 'desktop',
      launchAfterApply: true,
      agreedConfigPolicy: false,
      handleLaunch: vi.fn(),
    } as unknown as AppManagerContextType;
    let renderer: ReactTestRenderer | undefined;
    try {
      act(() => {
        renderer = create(
          <AppManagerContext.Provider value={context}>
            <AppManagerBottom />
          </AppManagerContext.Provider>
        );
      });
      const gears = renderer!.root
        .findAllByType('button')
        .filter((button) => button.props['aria-label'] === 'cliLaunch.title');
      expect(gears).toHaveLength(shown ? 1 : 0);
      expect(context.handleLaunch).not.toHaveBeenCalled();
    } finally {
      act(() => renderer?.unmount());
    }
  });

  it.each(accountTools)(
    '%s keeps the launch option available for the selected account',
    async (client, field) => {
      const { AppManagerBottom } = await import('./AppManagerComponents');
      const context = {
        selectedTool: client,
        selectedToolData: { ...tool, id: client },
        toolModelConfig: {},
        [field]: field === 'selectedCodexAccountId' ? 'saved' : { selectedId: 'saved' },
        viewMode: 'desktop',
        launchAfterApply: true,
        agreedConfigPolicy: false,
        setLaunchAfterApply: vi.fn(),
        setAgreedConfigPolicy: vi.fn(),
        handleLaunch: vi.fn(),
      } as unknown as AppManagerContextType;
      let renderer: ReactTestRenderer | undefined;
      const draw = () => (
        <AppManagerContext.Provider value={{ ...context }}>
          <AppManagerBottom />
        </AppManagerContext.Provider>
      );
      try {
        act(() => {
          renderer = create(draw());
        });
        const options = () => renderer!.root.findAllByType('label');
        expect(options()).toHaveLength(2);
        expect(options()[0].props.className).not.toContain('opacity-40');
        expect(options()[1].props.className).toContain('opacity-40 pointer-events-none');
        act(() => options().forEach((option) => option.props.onClick()));
        expect(context.setLaunchAfterApply).toHaveBeenCalledWith(false);
        expect(context.setAgreedConfigPolicy).not.toHaveBeenCalled();
        vi.mocked(context.setLaunchAfterApply).mockClear();

        // With launching off the primary action reads as modify-only and stays enabled.
        context.launchAfterApply = false;
        act(() => renderer!.update(draw()));
        const primary = renderer!.root.findAllByType('button')[0];
        expect(primary.props.disabled).toBe(false);
        expect(primary.props.children).toBe('btn.modifyOnly');
        context.launchAfterApply = true;

        Object.assign(context, {
          [field]: field === 'selectedCodexAccountId' ? null : { selectedId: null },
          toolModelConfig: { [client]: 'cloud-model' },
        });
        act(() => renderer!.update(draw()));
        expect(options()[0].props.className).not.toContain('opacity-40');
        expect(options()[1].props.className).not.toContain('opacity-40');

        // A tool without model configuration stays disabled even without an account.
        context.selectedToolData = { ...tool, id: client, noModelConfig: true };
        act(() => renderer!.update(draw()));
        expect(options()[0].props.className).toContain('opacity-40 pointer-events-none');
        expect(options()[1].props.className).toContain('opacity-40 pointer-events-none');
        act(() => options().forEach((option) => option.props.onClick()));
        expect(context.setLaunchAfterApply).not.toHaveBeenCalled();
        expect(context.setAgreedConfigPolicy).not.toHaveBeenCalled();

        // Leaving the account tool must not lock an unrelated API model or write preferences.
        Object.assign(context, {
          [field]: field === 'selectedCodexAccountId' ? 'saved' : { selectedId: 'saved' },
        });
        context.selectedTool = 'test-tool';
        context.selectedToolData = tool;
        context.toolModelConfig = { 'test-tool': 'cloud-model' };
        act(() => renderer!.update(draw()));
        expect(options()[0].props.className).not.toContain('opacity-40');
        expect(context.handleLaunch).not.toHaveBeenCalled();
        expect(context.setLaunchAfterApply).not.toHaveBeenCalled();
        expect(context.setAgreedConfigPolicy).not.toHaveBeenCalled();
        act(() => options().forEach((option) => option.props.onClick()));
        expect(context.setLaunchAfterApply).toHaveBeenCalledWith(false);
        expect(context.setAgreedConfigPolicy).toHaveBeenCalledWith(true);
      } finally {
        act(() => renderer?.unmount());
      }
    }
  );
});

describe('CodexAccountSection', () => {
  it('renders saved accounts with pending selection and current-state metadata', async () => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    const { CodexAccountSection } = await import('./AppManagerComponents');
    const context: Partial<AppManagerContextType> = {
      codexAccounts: [
        {
          id: 'account-1',
          email: 'first@example.com',
          plan: 'prolite',
          subscriptionEndAt: 1_800_086_400,
          quotaWindows: [
            { label: '5h', remainingPercent: 65, resetAt: 1_800_000_000 },
            { label: '7d', remainingPercent: 32, resetAt: 1_800_086_400 },
          ],
          quotaPercent: 32,
          quotaResetAt: 1_800_000_000,
          active: true,
        },
      ],
      selectedCodexAccountId: 'account-1',
      setSelectedCodexAccountId: () => {},
      isLoadingCodexAccounts: false,
      isAddingCodexAccount: false,
      codexOAuthRemainingSeconds: 0,
      refreshingCodexAccountIds: new Set(),
      codexAuthorizationFailedIds: new Set(),
      addCodexAccount: async () => {},
      refreshCodexAccountQuota: async () => undefined,
      deleteCodexAccount: async () => {},
    };

    const markup = renderToStaticMarkup(
      <AppManagerContext.Provider value={context as AppManagerContextType}>
        <CodexAccountSection showDivider={false} />
      </AppManagerContext.Provider>
    );

    expect(markup).toContain('first@example.com');
    expect(markup).toContain('Pro 100');
    expect(markup).toContain('65%');
    expect(markup).toContain('32%');
    expect(markup).not.toContain('5h 65%');
    expect(markup).not.toContain('7d 32%');
    expect(markup).not.toContain('account-quota-track');
    expect(markup).toContain('aria-checked="true"');
    expect(markup).toContain('agent.refreshAccount');
    expect(markup).not.toContain('role="tooltip"');
    expect(markup).not.toContain('border-b');
  });

  it('uses the progress bar for one quota window and puts the subscription before the plan', async () => {
    const { CodexAccountSection } = await import('./AppManagerComponents');
    const context: Partial<AppManagerContextType> = {
      codexAccounts: [
        {
          id: 'account-1',
          email: 'a@example.com',
          plan: 'prolite',
          subscriptionEndAt: 1_800_086_400,
          quotaWindows: [{ label: '7d', remainingPercent: 31, resetAt: 1_800_000_000 }],
          active: false,
        },
      ],
      selectedCodexAccountId: null,
      isLoadingCodexAccounts: false,
      isAddingCodexAccount: false,
      codexOAuthRemainingSeconds: 0,
      refreshingCodexAccountIds: new Set(),
      codexAuthorizationFailedIds: new Set(),
    };
    const markup = renderToStaticMarkup(
      <AppManagerContext.Provider value={context as AppManagerContextType}>
        <CodexAccountSection showDivider={false} />
      </AppManagerContext.Provider>
    );
    expect(markup).toContain('account-quota-track');
    expect(markup).toContain('31%');
    expect(markup).not.toContain('7d 31%');
    const row = renderToStaticMarkup(
      <AccountSectionRow
        selected={false}
        email="a@example.com"
        plan="Pro 100"
        planPrefix={<span>remaining</span>}
        onSelect={() => {}}
        onDelete={() => {}}
      />
    );
    expect(row.indexOf('remaining')).toBeLessThan(row.indexOf('Pro 100'));
  });

  it.each([
    ['free', 'Free'],
    ['go', 'Go'],
    ['plus', 'Plus'],
    ['pro', 'Pro 200'],
    ['prolite', 'Pro 100'],
    ['pro-5x', 'Pro 100'],
    ['pro-20x', 'Pro 200'],
    ['promax', 'Pro 500'],
    ['pro_100', 'Pro 100'],
    ['pro_200', 'Pro 200'],
    ['pro_500', 'Pro 500'],
    ['team', 'Business'],
    ['enterprise', 'Enterprise'],
  ])('labels %s as %s', async (plan, label) => {
    const { CodexAccountSection } = await import('./AppManagerComponents');
    const context: Partial<AppManagerContextType> = {
      codexAccounts: [{ id: 'account-1', email: 'a@example.com', plan, active: false }],
      selectedCodexAccountId: null,
      isLoadingCodexAccounts: false,
      isAddingCodexAccount: false,
      codexOAuthRemainingSeconds: 0,
      refreshingCodexAccountIds: new Set(),
      codexAuthorizationFailedIds: new Set(),
    };
    const markup = renderToStaticMarkup(
      <AppManagerContext.Provider value={context as AppManagerContextType}>
        <CodexAccountSection showDivider={false} />
      </AppManagerContext.Provider>
    );
    expect(markup).toContain(`>${label}</span>`);
  });

  it('reserves enough room for a long plan without a subscription countdown', async () => {
    const { CodexAccountSection } = await import('./AppManagerComponents');
    const context: Partial<AppManagerContextType> = {
      codexAccounts: [
        { id: 'account-1', email: 'long@example.com', plan: 'enterprise', active: false },
      ],
      selectedCodexAccountId: null,
      isLoadingCodexAccounts: false,
      isAddingCodexAccount: false,
      codexOAuthRemainingSeconds: 0,
      refreshingCodexAccountIds: new Set(),
      codexAuthorizationFailedIds: new Set(),
    };
    const markup = renderToStaticMarkup(
      <AppManagerContext.Provider value={context as AppManagerContextType}>
        <CodexAccountSection showDivider={false} />
      </AppManagerContext.Provider>
    );
    expect(markup).toContain('grid-cols-[minmax(0,1fr)_72px]');
    expect(markup).toContain('>Enterprise</span>');
  });
});

describe('AppManager views', () => {
  const installedTool = { ...tool, id: 'installed-app', name: 'Installed App' };
  const uninstalledTool = {
    ...tool,
    id: 'uninstalled-app',
    name: 'Uninstalled App',
    installed: false,
  };

  const renderView = async (viewMode: 'desktop' | 'install', detectedTools: LocalTool[]) => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    const { AppManagerMain } = await import('./AppManagerComponents');
    const context: Partial<AppManagerContextType> = {
      detectedTools,
      viewMode,
      isScanning: false,
      selectedTool: null,
      setSelectedTool: () => {},
      aiInstallableIds: ['uninstalled-app'],
    };
    return renderToStaticMarkup(
      <AppManagerContext.Provider value={context as AppManagerContextType}>
        <AppManagerMain />
      </AppManagerContext.Provider>
    );
  };

  it.each([
    ['desktop', 'Installed App', 'Uninstalled App'],
    ['install', 'Uninstalled App', 'Installed App'],
  ] as const)('%s shows only the matching apps', async (mode, visible, hidden) => {
    const markup = await renderView(mode, [installedTool, uninstalledTool]);
    expect(markup).toContain(`aria-label="${visible}"`);
    expect(markup).not.toContain(`aria-label="${hidden}"`);
  });

  it('hides bundled app entries while retaining installed plugin entries', async () => {
    const markup = await renderView('desktop', [
      { ...installedTool, id: 'reversi', name: 'Bundled board game' },
      { ...installedTool, id: 'translator', name: 'AI Translator' },
      {
        ...installedTool,
        id: 'plugin:fixture.game:game',
        name: 'Plugin board game',
        plugin: { id: 'fixture.game', pageId: 'game' },
      },
    ]);
    expect(markup).not.toContain('aria-label="Bundled board game"');
    expect(markup).not.toContain('aria-label="AI Translator"');
    expect(markup).toContain('aria-label="Plugin board game"');
  });

  it.each([
    ['desktop', [uninstalledTool], 'aiDesktop.emptyDesktop'],
    ['install', [installedTool], 'aiDesktop.emptyInstall'],
  ] as const)('%s explains an empty list', async (mode, tools, message) => {
    const markup = await renderView(mode, [...tools]);
    expect(markup).toContain(message);
  });
});

describe('PageAwareHint', () => {
  let renderer: ReactTestRenderer | undefined;

  beforeEach(() => {
    vi.spyOn(Math, 'random').mockReturnValue(0.25);
  });

  afterEach(() => {
    act(() => renderer?.unmount());
    renderer = undefined;
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  const renderHint = async (selectedTool: string | null, selectedAccount: string | null = null) => {
    vi.stubGlobal('__APP_EDITION__', 'full');
    useNavigationStore.setState({ activePage: 'apps' });
    const { PageAwareHint } = await import('./AppManagerComponents');
    const context: Partial<AppManagerContextType> = {
      viewMode: 'desktop',
      selectedTool,
      claudeCodeAccounts: {
        selectedId: selectedAccount,
      } as AppManagerContextType['claudeCodeAccounts'],
      claudeDesktopAccounts: {
        selectedId: selectedAccount,
      } as AppManagerContextType['claudeDesktopAccounts'],
    };
    return renderToStaticMarkup(
      <AppManagerContext.Provider value={context as AppManagerContextType}>
        <PageAwareHint />
      </AppManagerContext.Provider>
    );
  };

  it.each([
    ['claudedesktop', 'hint.claudeHistoryMerged'],
    ['claudecode', 'hint.appReorder'],
  ])('shows the selected app hint for %s API models', async (toolId, hint) => {
    const markup = await renderHint(toolId);
    expect(markup).toContain(hint);
  });

  it.each(['chatgptdesktop', 'codex'])(
    'shows the Responses compatibility reminder for %s',
    async (toolId) => {
      const markup = await renderHint(toolId);
      expect(markup).toContain('hint.responsesRequired');
      expect(markup).toContain('hint.localHistoryPreserved');
      expect(markup).not.toContain('hint.appReorder');
    }
  );

  it('shows the app reorder hint when a Claude Code account is selected', async () => {
    const markup = await renderHint('claudecode', 'saved-account');
    expect(markup).toContain('hint.appReorder');
  });
  it('shows the Claude Code history hint when a Claude Desktop account is selected', async () => {
    const markup = await renderHint('claudedesktop', 'saved-account');
    expect(markup).toContain('hint.claudeHistoryMerged');
    expect(markup).not.toContain('hint.localHistoryPreserved');
  });

  it('shows the app reorder hint for other tools', async () => {
    const markup = await renderHint('test-tool');
    expect(markup).toContain('hint.appReorder');
  });

  it.each([
    [0, 'hint.cliRecommendation'],
    [0.25, 'hint.appReorder'],
    [0.5, 'hint.smartAccess'],
    [0.75, 'hint.voiceInput'],
    [0.99, 'hint.voiceSelection'],
  ] as const)('randomly selects %s on opening', async (random, hint) => {
    vi.mocked(Math.random).mockReturnValue(random);
    expect(await renderHint('test-tool')).toContain(hint);
  });

  it('rotates every 10 minutes without repeating and stops when hidden or unmounted', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('__APP_EDITION__', 'full');
    useNavigationStore.setState({ activePage: 'apps' });
    const { PageAwareHint } = await import('./AppManagerComponents');
    const render = (selectedTool = 'test-tool', viewMode = 'desktop') => (
      <AppManagerContext.Provider value={{ selectedTool, viewMode } as AppManagerContextType}>
        <PageAwareHint />
      </AppManagerContext.Provider>
    );
    act(() => {
      renderer = create(render());
    });
    const text = () => (renderer!.toJSON() as ReactTestRendererJSON).children?.join('');
    expect(text()).toBe('hint.appReorder');
    expect(vi.getTimerCount()).toBe(1);
    act(() => {
      vi.advanceTimersByTime(599_999);
    });
    expect(text()).toBe('hint.appReorder');
    for (const [index, hint] of [
      'hint.voiceInput',
      'hint.cliRecommendation',
      'hint.smartAccess',
      'hint.voiceSelection',
      'hint.appReorder',
    ].entries()) {
      act(() => {
        vi.advanceTimersByTime(index === 0 ? 1 : 600_000);
      });
      expect(text()).toBe(hint);
    }

    act(() => renderer!.update(render('claudecode')));
    expect(text()).toBe('hint.appReorder');
    expect(vi.getTimerCount()).toBe(1);

    for (const toolId of ['chatgptdesktop', 'codex', 'claudedesktop', 'workbuddy', 'workbuddyai']) {
      act(() => renderer!.update(render(toolId)));
      expect(vi.getTimerCount()).toBe(0);
    }
    act(() => renderer!.update(render('test-tool', 'install')));
    expect(text()).toBe('aiDesktop.installHint');
    expect(vi.getTimerCount()).toBe(0);
    act(() => renderer!.update(render()));
    expect(vi.getTimerCount()).toBe(1);
    act(() =>
      useNavigationStore.setState({
        activePage: 'developer',
        developerMode: true,
        developerTab: 'plugins',
      })
    );
    expect(text()).toBeUndefined();
    expect(vi.getTimerCount()).toBe(0);
    act(() => useNavigationStore.setState({ activePage: 'voice' }));
    expect(vi.getTimerCount()).toBe(0);

    vi.mocked(Math.random).mockReturnValue(0.99);
    act(() => useNavigationStore.setState({ activePage: 'apps' }));
    expect(text()).toBe('hint.voiceSelection');
    expect(vi.getTimerCount()).toBe(1);
    act(() => renderer!.unmount());
    renderer = undefined;
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe.each(['codex', 'chatgptdesktop'])('%s Web Search control', (client) => {
  it.each(
    [
      { installed: true, account: null, pending: false, hasModels: true },
      { installed: true, account: null, pending: true, hasModels: true },
      { installed: true, account: 'account', pending: false, hasModels: false },
      { installed: false, account: null, pending: false, hasModels: false },
    ].flatMap((state) => [false, true].map((webSearch) => ({ ...state, webSearch })))
  )(
    'restores visibility for the selected tool and disables pending changes: %j',
    async ({ installed, account, pending, hasModels, webSearch }) => {
      const { AppManagerPanel } = await import('./AppManagerComponents');
      const { ModelNexusContext } = await import('../ModelNexus/context');
      const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
      const context = {
        selectedTool: client,
        selectedToolData: { ...tool, id: client, installed },
        userModels: hasModels ? models : [],
        toolModelConfig: hasModels ? { [client]: 'smart-router' } : {},
        codexAccounts: [],
        selectedCodexAccountId: account,
        refreshingCodexAccountIds: new Set(),
        codexAuthorizationFailedIds: new Set(),
        codexWebSearch: webSearch,
        isLaunching: pending,
      } as unknown as AppManagerContextType;
      const markup = renderToStaticMarkup(
        <ConfirmDialogProvider>
          <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
            <AppManagerContext.Provider value={context}>
              <AppManagerPanel />
            </AppManagerContext.Provider>
          </ModelNexusContext.Provider>
        </ConfirmDialogProvider>
      );
      const toggle = markup.match(/<button[^>]*aria-pressed="[^>]*>/)?.[0];
      if (installed) {
        expect(toggle).toContain('aria-label="agent.codexWebSearchLabel"');
        expect(toggle).toContain(`aria-pressed="${webSearch}"`);
        expect(toggle?.includes('disabled=""')).toBe(pending);
        expect(markup).toContain('agent.codexWebSearchHint');
        expect(markup.indexOf('agent.modelSwitch')).toBeLessThan(markup.indexOf('aria-pressed='));
        if (hasModels)
          expect(markup.indexOf('aria-pressed=')).toBeLessThan(markup.indexOf('Cloud Model'));
      } else {
        expect(toggle).toBeUndefined();
      }
      expect(markup).not.toContain('agent.codexRelayLabel');
      expect(markup).not.toContain('role="tooltip"');
      expect(markup).not.toContain('cursor-help');
    }
  );
});

describe('ZCode model capability controls', () => {
  it.each([false, true])(
    'reuses the switches and disables both while pending=%s',
    async (pending) => {
      const { AppManagerPanel } = await import('./AppManagerComponents');
      const { ModelNexusContext } = await import('../ModelNexus/context');
      const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
      const context = {
        selectedTool: 'zcode',
        selectedToolData: { ...tool, id: 'zcode' },
        toolModelConfig: {},
        userModels: models,
        zcodeWebSearch: false,
        zcodeImageInput: true,
        isLaunching: pending,
        zcodeAccounts: {
          accounts: [],
          selectedId: null,
          refreshing: new Set(),
          authorizationFailedIds: new Set(),
        },
      } as unknown as AppManagerContextType;
      const markup = renderToStaticMarkup(
        <ConfirmDialogProvider>
          <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
            <AppManagerContext.Provider value={context}>
              <AppManagerPanel />
            </AppManagerContext.Provider>
          </ModelNexusContext.Provider>
        </ConfirmDialogProvider>
      );
      for (const [label, checked] of [
        ['agent.codexWebSearchLabel', false],
        ['Image', true],
      ]) {
        const toggle = markup.match(new RegExp(`<button[^>]*aria-label="${label}"[^>]*>`))?.[0];
        expect(toggle).toContain(`aria-pressed="${checked}"`);
        expect(toggle?.includes('disabled=""')).toBe(pending);
      }
      expect(markup).not.toContain('role="tooltip"');
      expect(markup).toContain('agent.codexWebSearchHint');
      expect(markup).toContain('agent.imageInputHint');
      expect(markup).not.toContain('aria-label="1M"');
    }
  );
});

describe.each(['claudedesktop', 'claudecode'])('%s model controls', (client) => {
  it.each([
    { selectedAccount: null, pending: false },
    { selectedAccount: 'saved-account', pending: false },
    { selectedAccount: null, pending: true },
  ])(
    'keeps 1M and removes API Router regardless of account selection: %j',
    async ({ selectedAccount, pending }) => {
      const { AppManagerPanel } = await import('./AppManagerComponents');
      const { ModelNexusContext } = await import('../ModelNexus/context');
      const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
      const accounts = {
        accounts: selectedAccount
          ? [
              {
                id: selectedAccount,
                email: 'saved@example.test',
                plan: null,
                fiveHour: null,
                sevenDay: null,
              },
            ]
          : [],
        selectedId: selectedAccount,
        busy: false,
        remainingSeconds: 0,
        refreshing: new Set(),
        authorizationFailedIds: new Set(),
        add: async () => {},
        select: () => {},
        refresh: async () => {},
        remove: async () => {},
      };
      const context = {
        selectedTool: client,
        selectedToolData: { ...tool, id: client },
        toolModelConfig: {},
        userModels: [],
        claudeDesktop1mMode: true,
        claude1mMode: true,
        claudeDesktopWebSearch: false,
        claudeCodeWebSearch: false,
        isLaunching: pending,
        claudeCodeAccounts: accounts,
        claudeDesktopAccounts: accounts,
      } as unknown as AppManagerContextType;
      const markup = renderToStaticMarkup(
        <ConfirmDialogProvider>
          <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
            <AppManagerContext.Provider value={context}>
              <AppManagerPanel />
            </AppManagerContext.Provider>
          </ModelNexusContext.Provider>
        </ConfirmDialogProvider>
      );
      expect(markup).not.toContain('agent.codexRelayLabel');
      expect(markup).not.toContain('agent.codexRelayHint');
      expect(markup).toMatch(/aria-pressed="true" aria-label="1M"/);
      expect(markup).toContain('agent.claude1mHint');
      expect(markup).toMatch(/aria-pressed="false" aria-label="agent.codexWebSearchLabel"/);
      const search = markup.match(/<button[^>]*aria-label="agent.codexWebSearchLabel"[^>]*>/)?.[0];
      expect(search?.includes('disabled=""')).toBe(pending);
      if (client === 'claudedesktop') expect(markup).toContain('Anthropic Official');
      if (selectedAccount) expect(markup).toContain('<div role="radio" aria-checked="true"');
    }
  );
});

describe.each(['grokbot', 'cursor', 'manus', 'cue'] as const)('%s account-only panel', (client) => {
  it('shows accounts for a no-model-config tool without rendering API models', async () => {
    const { AppManagerPanel, AppManagerBottom } = await import('./AppManagerComponents');
    const { ModelNexusContext } = await import('../ModelNexus/context');
    const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
    const context = {
      selectedTool: client,
      selectedToolData: { ...tool, id: client, name: client, noModelConfig: true },
      userModels: models,
      toolModelConfig: { [client]: 'cloud-model' },
      [client === 'cursor'
        ? 'cursorAccounts'
        : client === 'manus'
          ? 'manusAccounts'
          : client === 'cue'
            ? 'cueAccounts'
            : 'grokBotAccounts']: {
        accounts: [
          {
            id: 'one',
            email: 'bot@example.test',
            active: true,
            credits: client === 'manus' || client === 'cue' ? { total: 1300 } : undefined,
          },
        ],
        selectedId: 'one',
        busy: true,
        remainingSeconds: 60,
        refreshing: new Set(),
        authorizationFailedIds: new Set(),
      },
      viewMode: 'desktop',
      launchAfterApply: false,
      isLaunching: false,
    } as unknown as AppManagerContextType;
    const markup = renderToStaticMarkup(
      <ConfirmDialogProvider>
        <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
          <AppManagerContext.Provider value={context}>
            <AppManagerPanel />
            <AppManagerBottom />
          </AppManagerContext.Provider>
        </ModelNexusContext.Provider>
      </ConfirmDialogProvider>
    );
    expect(markup).toContain('bot@example.test');
    if (client === 'manus' || client === 'cue') expect(markup).toContain('1300 agent.credits');
    expect(markup).not.toContain('Cloud Model');
    expect(markup).not.toContain('Auto Router');
    expect(markup).not.toContain('agent.noModelConfig');
    expect(markup).toContain('btn.modifyOnly');
    expect(markup.match(/<button[^>]*disabled=""/g)).toHaveLength(1);
    expect(markup).toContain('aria-label="btn.cancel"');
  });
});

describe.each(['workbuddy', 'workbuddyai'])('%s image control', (client) => {
  it.each([false, true])('reuses the image switch with pending=%s', async (pending) => {
    const { AppManagerPanel } = await import('./AppManagerComponents');
    const { ModelNexusContext } = await import('../ModelNexus/context');
    const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
    const context = {
      selectedTool: client,
      selectedToolData: { ...tool, id: client },
      toolModelConfig: {},
      userModels: models,
      workBuddyImageInput: false,
      workBuddy1mMode: true,
      isLaunching: pending,
      workBuddyAccounts: {
        accounts: [],
        selectedId: null,
        refreshing: new Set(),
        authorizationFailedIds: new Set(),
      },
    } as unknown as AppManagerContextType;
    const markup = renderToStaticMarkup(
      <ConfirmDialogProvider>
        <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
          <AppManagerContext.Provider value={context}>
            <AppManagerPanel />
          </AppManagerContext.Provider>
        </ModelNexusContext.Provider>
      </ConfirmDialogProvider>
    );
    const toggle = markup.match(/<button[^>]*aria-label="Image"[^>]*>/)?.[0];
    expect(toggle).toContain('aria-pressed="false"');
    expect(toggle?.includes('disabled=""')).toBe(pending);
    expect(markup).toContain('agent.imageInputHint');
    expect(markup).toContain('agent.oneMContextHint');
    expect(markup).toMatch(/aria-pressed="true" aria-label="1M"/);
    expect(markup).not.toContain('agent.codexWebSearchLabel');
    expect(markup).not.toContain('role="tooltip"');
  });
});

describe.each(['mimocode', 'mimodesktop'])('%s capability controls', (client) => {
  it.each([false, true])('reuses image and 1M switches with pending=%s', async (pending) => {
    const { AppManagerPanel } = await import('./AppManagerComponents');
    const { ModelNexusContext } = await import('../ModelNexus/context');
    const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
    const context = {
      selectedTool: client,
      selectedToolData: { ...tool, id: client },
      toolModelConfig: {},
      userModels: models,
      mimoImageInput: true,
      mimo1mMode: true,
      isLaunching: pending,
    } as unknown as AppManagerContextType;
    const markup = renderToStaticMarkup(
      <ConfirmDialogProvider>
        <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
          <AppManagerContext.Provider value={context}>
            <AppManagerPanel />
          </AppManagerContext.Provider>
        </ModelNexusContext.Provider>
      </ConfirmDialogProvider>
    );
    for (const label of ['Image', '1M']) {
      const toggle = markup.match(new RegExp(`<button[^>]*aria-label="${label}"[^>]*>`))?.[0];
      expect(toggle).toContain('aria-pressed="true"');
      expect(toggle?.includes('disabled=""')).toBe(pending);
    }
    expect(markup).not.toContain('agent.claude1mHint');
    expect(markup).toContain('agent.oneMContextHint');
    expect(markup).toContain('agent.imageInputHint');
    expect(markup).not.toContain('agent.codexWebSearchLabel');
    expect(markup).not.toContain('role="tooltip"');
  });
});

describe.each(['minimaxcode', 'minimaxdesktop'])('%s capability controls', (client) => {
  it.each([false, true])('reuses image and 1M switches with pending=%s', async (pending) => {
    const { AppManagerPanel } = await import('./AppManagerComponents');
    const { ModelNexusContext } = await import('../ModelNexus/context');
    const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
    const context = {
      selectedTool: client,
      selectedToolData: { ...tool, id: client },
      toolModelConfig: {},
      userModels: models,
      minimaxImageInput: true,
      minimax1mMode: true,
      minimaxAccounts: {
        accounts: [],
        selectedId: null,
        refreshing: new Set(),
        authorizationFailedIds: new Set(),
      },
      isLaunching: pending,
    } as unknown as AppManagerContextType;
    const markup = renderToStaticMarkup(
      <ConfirmDialogProvider>
        <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
          <AppManagerContext.Provider value={context}>
            <AppManagerPanel />
          </AppManagerContext.Provider>
        </ModelNexusContext.Provider>
      </ConfirmDialogProvider>
    );
    for (const label of ['Image', '1M']) {
      const toggle = markup.match(new RegExp(`<button[^>]*aria-label="${label}"[^>]*>`))?.[0];
      expect(toggle).toContain('aria-pressed="true"');
      expect(toggle?.includes('disabled=""')).toBe(pending);
    }
    expect(markup).not.toContain('agent.claude1mHint');
    expect(markup).toContain('agent.oneMContextHint');
    expect(markup).toContain('agent.imageInputHint');
    expect(markup).not.toContain('agent.codexWebSearchLabel');
    expect(markup).not.toContain('role="tooltip"');
  });
});

describe.each(['qoder', 'qodercn'])('%s capability controls', (client) => {
  it.each([false, true])('reuses image and 1M switches with pending=%s', async (pending) => {
    const { AppManagerPanel } = await import('./AppManagerComponents');
    const { ModelNexusContext } = await import('../ModelNexus/context');
    const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
    const context = {
      selectedTool: client,
      selectedToolData: { ...tool, id: client },
      toolModelConfig: {},
      userModels: models,
      qoderImageInput: true,
      qoder1mMode: true,
      qoderAccounts: {
        accounts: [],
        selectedId: null,
        refreshing: new Set(),
        authorizationFailedIds: new Set(),
      },
      isLaunching: pending,
    } as unknown as AppManagerContextType;
    const markup = renderToStaticMarkup(
      <ConfirmDialogProvider>
        <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
          <AppManagerContext.Provider value={context}>
            <AppManagerPanel />
          </AppManagerContext.Provider>
        </ModelNexusContext.Provider>
      </ConfirmDialogProvider>
    );
    for (const label of ['Image', '1M']) {
      const toggle = markup.match(new RegExp(`<button[^>]*aria-label="${label}"[^>]*>`))?.[0];
      expect(toggle).toContain('aria-pressed="true"');
      expect(toggle?.includes('disabled=""')).toBe(pending);
    }
    expect(markup).not.toContain('agent.claude1mHint');
    expect(markup).toContain('agent.oneMContextHint');
    expect(markup).toContain('agent.imageInputHint');
    expect(markup).not.toContain('agent.codexWebSearchLabel');
    expect(markup).not.toContain('role="tooltip"');
  });
});

describe.each(['dsh'])('%s capability controls', (client) => {
  it.each([false, true])('reuses image and 1M switches with pending=%s', async (pending) => {
    const { AppManagerPanel } = await import('./AppManagerComponents');
    const { ModelNexusContext } = await import('../ModelNexus/context');
    const { ConfirmDialogProvider } = await import('../../components/ConfirmDialog');
    const context = {
      selectedTool: client,
      selectedToolData: { ...tool, id: client },
      toolModelConfig: {},
      userModels: models,
      dshImageInput: true,
      dsh1mMode: true,
      deepSeekAccounts: {
        accounts: [],
        selectedId: null,
        refreshing: new Set(),
        authorizationFailedIds: new Set(),
      },
      isLaunching: pending,
    } as unknown as AppManagerContextType;
    const markup = renderToStaticMarkup(
      <ConfirmDialogProvider>
        <ModelNexusContext.Provider value={{} as React.ContextType<typeof ModelNexusContext>}>
          <AppManagerContext.Provider value={context}>
            <AppManagerPanel />
          </AppManagerContext.Provider>
        </ModelNexusContext.Provider>
      </ConfirmDialogProvider>
    );
    for (const label of ['Image', '1M']) {
      const toggle = markup.match(new RegExp(`<button[^>]*aria-label="${label}"[^>]*>`))?.[0];
      expect(toggle).toContain('aria-pressed="true"');
      expect(toggle?.includes('disabled=""')).toBe(pending);
    }
    expect(markup).not.toContain('agent.claude1mHint');
    expect(markup).toContain('agent.oneMContextHint');
    expect(markup).toContain('agent.imageInputHint');
    expect(markup).not.toContain('agent.codexWebSearchLabel');
    expect(markup).not.toContain('role="tooltip"');
  });
});
