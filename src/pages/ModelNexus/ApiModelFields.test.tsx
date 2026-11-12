import { useLayoutEffect, useState, type Dispatch, type SetStateAction } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { readText } from '@tauri-apps/plugin-clipboard-manager';
import * as api from '../../api/models';
import { ModelIdCombobox } from '../../components/ModelIdCombobox';
import { ApiModelFields } from './ApiModelFields';
import type { NewModelForm } from './context';

const { showToast, i18n } = vi.hoisted(() => ({
  showToast: vi.fn(),
  i18n: { t: (key: string) => key },
}));
vi.mock('../../hooks/useI18n', () => ({ useI18n: () => ({ t: i18n.t }) }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../../components/ModelIdCombobox', () => ({ ModelIdCombobox: () => null }));
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ readText: vi.fn() }));
vi.mock('../../api/models', async (original) => ({
  ...(await original<typeof import('../../api/models')>()),
  getCachedUpstreamModelCatalog: vi.fn(),
  fetchUpstreamModelCatalog: vi.fn(),
  getModels: vi.fn(),
  queryModelUsage: vi.fn(),
  addModel: vi.fn(),
  updateModel: vi.fn(),
  testModel: vi.fn(),
  pingModel: vi.fn(),
}));

const source = {
  baseUrl: 'https://models.fixture.test/v1',
  anthropicUrl: '',
  apiKey: '',
};
const directory: api.UpstreamModelCatalog = {
  models: [
    { id: 'free-fixture', free: true },
    { id: 'paid-fixture', multiplier: 0.75 },
    { id: 'unknown-fixture' },
  ],
  requiresApiKey: false,
};
let renderer: ReactTestRenderer | undefined;
let current: NewModelForm;
let updateForm: Dispatch<SetStateAction<NewModelForm>>;
let edits: NewModelForm[];

function Harness({
  initial,
  editing = false,
  active = true,
}: {
  initial: NewModelForm;
  editing?: boolean;
  active?: boolean;
}) {
  const [form, setForm] = useState(initial);
  useLayoutEffect(() => {
    current = form;
    updateForm = setForm;
    edits.push(form);
  }, [form]);
  return <ApiModelFields form={form} setForm={setForm} editing={editing} active={active} />;
}

function initialForm(patch: Partial<NewModelForm> = {}): NewModelForm {
  return { name: 'Fixture', ...source, modelId: 'manual-fixture', ...patch };
}
async function open(
  patch: Partial<NewModelForm> = {},
  props: { editing?: boolean; active?: boolean } = {}
) {
  await act(async () => {
    renderer = create(<Harness initial={initialForm(patch)} {...props} />);
  });
}
function field() {
  return renderer!.root.findByType(ModelIdCombobox);
}
async function advance(ms = 800) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}
async function patchForm(patch: Partial<NewModelForm>) {
  await act(async () => updateForm((form) => ({ ...form, ...patch })));
}
async function refresh() {
  await act(async () => field().props.onRefresh());
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.useFakeTimers();
  edits = [];
  i18n.t = (key: string) => key;
  renderer = undefined;
  vi.mocked(api.getCachedUpstreamModelCatalog).mockReset().mockResolvedValue(null);
  vi.mocked(api.fetchUpstreamModelCatalog).mockReset().mockResolvedValue(directory);
  vi.mocked(readText).mockReset().mockResolvedValue('');
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.useRealTimers();
  vi.clearAllMocks();
});

it('reads local cache immediately, then automatically refreshes Add after the debounce', async () => {
  vi.mocked(api.getCachedUpstreamModelCatalog).mockResolvedValue({
    models: [{ id: 'cached-fixture' }],
    requiresApiKey: false,
  });
  await open({ modelIdOptions: ['fallback-fixture'] });
  expect(api.getCachedUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith(source);
  expect(field().props.options).toEqual(['cached-fixture']);
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
  await advance(799);
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
  await advance(1);
  expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith(source);
  expect(field().props.options).toEqual(directory.models.map((model) => model.id));
  expect(field().props.freeModelIds).toEqual(['free-fixture']);
  expect(field().props.optionMultipliers).toEqual({ 'paid-fixture': 0.75 });
  expect(current.modelId).toBe('manual-fixture');
  expect(showToast).not.toHaveBeenCalled();
});

it('selects the first upstream ID only when the Add form has no current model ID', async () => {
  await open({ modelId: '' });
  await advance();
  expect(current.modelId).toBe('free-fixture');
  await act(async () => field().props.onChange('my-custom-model'));
  await refresh();
  expect(current.modelId).toBe('my-custom-model');
  expect(field().props.value).toBe('my-custom-model');
});

it('defaults an empty Add to the first free choice using name or directory metadata', async () => {
  vi.mocked(api.fetchUpstreamModelCatalog).mockResolvedValue({
    models: [
      { id: 'paid-first' },
      { id: 'blocked:free', free: false },
      { id: 'charged-free', multiplier: 2 },
      { id: 'provider/Model:Free' },
      { id: 'zero-multiplier', multiplier: 0 },
    ],
    requiresApiKey: false,
  });
  await open({ modelId: '' });
  await advance();
  expect(current.modelId).toBe('provider/Model:Free');
  await act(async () => renderer!.unmount());
  vi.mocked(api.fetchUpstreamModelCatalog).mockResolvedValue({
    models: [{ id: 'paid-first' }, { id: 'big-pickle' }, { id: 'provider/model-free' }],
    requiresApiKey: false,
  });
  await open({ modelId: '', freeModelIds: ['big-pickle'] });
  await advance();
  expect(current.modelId).toBe('big-pickle');
});

it('opens Edit from its local cache and queries upstream only after explicit refresh', async () => {
  vi.mocked(api.getCachedUpstreamModelCatalog).mockResolvedValue(directory);
  await open({}, { editing: true });
  await advance(5000);
  expect(field().props.options).toEqual(directory.models.map((model) => model.id));
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
  await refresh();
  expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith(source);
  expect(current.modelId).toBe('manual-fixture');
  expect(api.getModels).not.toHaveBeenCalled();
  expect(api.queryModelUsage).not.toHaveBeenCalled();
  expect(api.addModel).not.toHaveBeenCalled();
  expect(api.updateModel).not.toHaveBeenCalled();
  expect(api.testModel).not.toHaveBeenCalled();
  expect(api.pingModel).not.toHaveBeenCalled();
});

it('carries known free directory metadata only for its original endpoint and honors paid overrides', async () => {
  vi.mocked(api.getCachedUpstreamModelCatalog).mockResolvedValue({
    models: [
      { id: 'big-pickle' },
      { id: 'renamed-free', free: false },
      { id: 'metered-alias', multiplier: 0.5 },
      { id: 'explicit-free', free: true, multiplier: 0.25 },
    ],
    requiresApiKey: false,
  });
  await open({ freeModelIds: ['big-pickle', 'renamed-free', 'metered-alias'] });
  expect(field().props.freeModelIds).toEqual(['explicit-free', 'big-pickle']);
  expect(field().props.paidModelIds).toEqual(['renamed-free']);
  expect(field().props.options).toEqual([
    'big-pickle',
    'renamed-free',
    'metered-alias',
    'explicit-free',
  ]);
  vi.mocked(api.getCachedUpstreamModelCatalog).mockResolvedValue(null);
  await patchForm({ baseUrl: 'https://another.fixture.test/v1' });
  expect(field().props.freeModelIds).toBeUndefined();
  expect(field().props.paidModelIds).toBeUndefined();
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
});

it('keeps an empty saved Edit model ID unchanged when loading cached options', async () => {
  vi.mocked(api.getCachedUpstreamModelCatalog).mockResolvedValue(directory);
  await open({ modelId: '' }, { editing: true });
  expect(field().props.options).toEqual(directory.models.map((model) => model.id));
  expect(current.modelId).toBe('');
  await refresh();
  expect(current.modelId).toBe('');
});

it('prompts for a required key and hides the prompt before automatically reading the keyed directory', async () => {
  vi.mocked(api.fetchUpstreamModelCatalog).mockResolvedValueOnce({
    models: [],
    requiresApiKey: true,
  });
  await open({ modelIdOptions: ['fallback-fixture'] });
  await advance();
  expect(field().props.actionHint).toBe('model.catalogKeyRequired');
  expect(field().props.options).toEqual(['fallback-fixture']);
  expect(current.modelId).toBe('manual-fixture');
  await patchForm({ apiKey: 'fixture-key' });
  expect(field().props.actionHint).toBeUndefined();
  expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledTimes(1);
  await advance();
  expect(api.fetchUpstreamModelCatalog).toHaveBeenLastCalledWith({
    ...source,
    apiKey: 'fixture-key',
  });
  expect(field().props.options).toEqual(directory.models.map((model) => model.id));
  expect(showToast).not.toHaveBeenCalled();
});

it('keeps the missing-key hint when an unkeyed catalog request fails, then retries with the key', async () => {
  vi.mocked(api.fetchUpstreamModelCatalog).mockRejectedValueOnce('UPSTREAM_MODELS_UNSUPPORTED');
  await open({ modelIdOptions: ['fallback-fixture'] });
  expect(field().props.actionHint).toBe('model.catalogKeyRequired');
  await advance();
  expect(field().props.actionHint).toBe('model.catalogKeyRequired');
  expect(field().props.options).toEqual(['fallback-fixture']);
  await patchForm({ apiKey: 'fixture-key' });
  expect(field().props.actionHint).toBeUndefined();
  await advance();
  expect(api.fetchUpstreamModelCatalog).toHaveBeenLastCalledWith({
    ...source,
    apiKey: 'fixture-key',
  });
  expect(field().props.options).toEqual(directory.models.map((model) => model.id));
});

it('removes the provisional key hint when an endpoint provides a keyless catalog', async () => {
  await open();
  expect(field().props.actionHint).toBe('model.catalogKeyRequired');
  await advance();
  expect(field().props.actionHint).toBeUndefined();
});

it('treats the no-key sentinel as empty for a key-required directory', async () => {
  vi.mocked(api.getCachedUpstreamModelCatalog).mockResolvedValue({
    models: [],
    requiresApiKey: true,
  });
  await open({ apiKey: 'not-needed' }, { editing: true });
  expect(field().props.actionHint).toBe('model.catalogKeyRequired');
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
});

it('uses authenticated catalog entries even when the upstream reports that a key is required', async () => {
  vi.mocked(api.fetchUpstreamModelCatalog).mockResolvedValue({
    ...directory,
    requiresApiKey: true,
  });
  await open({ apiKey: 'fixture-key', modelId: '', modelIdOptions: ['fallback-fixture'] });
  await advance();
  expect(field().props.actionHint).toBeUndefined();
  expect(field().props.options).toEqual(directory.models.map((model) => model.id));
  expect(current.modelId).toBe('free-fixture');
});

it.each([
  { editing: false, modelId: '', expected: 'stable-preset' },
  { editing: false, modelId: 'my-manual-model', expected: 'my-manual-model' },
  { editing: true, modelId: '', expected: '' },
])(
  'uses a preset only for unsupported discovery, preserving $modelId in Edit=$editing',
  async ({ editing, modelId, expected }) => {
    vi.mocked(api.fetchUpstreamModelCatalog).mockRejectedValue('UPSTREAM_MODELS_UNSUPPORTED');
    await open({ modelId, modelIdOptions: ['stable-preset', 'other-preset'] }, { editing });
    if (editing) await refresh();
    else await advance();
    expect(current.modelId).toBe(expected);
    expect(field().props.options).toEqual(['stable-preset', 'other-preset']);
    if (editing)
      expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'model.catalogUnsupported');
    else expect(showToast).not.toHaveBeenCalled();
    expect(api.addModel).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(api.queryModelUsage).not.toHaveBeenCalled();
  }
);

it('does not restart automatic loading when only the display language changes', async () => {
  await open();
  await advance(400);
  i18n.t = (key: string) => `localized.${key}`;
  await act(async () => renderer!.update(<Harness initial={initialForm()} />));
  await advance(400);
  expect(api.getCachedUpstreamModelCatalog).toHaveBeenCalledOnce();
  expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledOnce();
  expect(field().props.freeLabel).toBe('localized.model.free');
});

it('shows the latest translation for a pending failure without repeating the Edit cache or request', async () => {
  const request = deferred<api.UpstreamModelCatalog>();
  vi.mocked(api.fetchUpstreamModelCatalog).mockReturnValue(request.promise);
  await open({}, { editing: true });
  await refresh();
  i18n.t = (key: string) => `localized.${key}`;
  await act(async () => renderer!.update(<Harness initial={initialForm()} editing />));
  await act(async () => request.reject('UPSTREAM_MODELS_HTTP_503'));
  expect(api.getCachedUpstreamModelCatalog).toHaveBeenCalledOnce();
  expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledOnce();
  expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'localized.model.catalogFailed');
});

it('debounces Add key and URL typing and uses only the completed source', async () => {
  await open();
  await advance(400);
  await patchForm({ apiKey: 'partial' });
  await advance(400);
  await patchForm({ apiKey: 'completed-key', baseUrl: 'https://other.fixture.test/v1' });
  await advance(799);
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
  await advance(1);
  expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith({
    ...source,
    baseUrl: 'https://other.fixture.test/v1',
    apiKey: 'completed-key',
  });
});

it('clears source-specific options and labels after a key or URL changes in Edit without querying upstream', async () => {
  vi.mocked(api.getCachedUpstreamModelCatalog).mockResolvedValueOnce(directory);
  await open({}, { editing: true });
  expect(field().props.freeModelIds).toEqual(['free-fixture']);
  await patchForm({ apiKey: 'other-key' });
  expect(field().props.options).toEqual([]);
  expect(field().props.freeModelIds).toBeUndefined();
  expect(field().props.optionMultipliers).toEqual({});
  await patchForm({ baseUrl: 'https://other.fixture.test/v1' });
  await advance(5000);
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
  expect(current.modelId).toBe('manual-fixture');
});

it('preserves the cached directory and selected ID when a manual refresh fails', async () => {
  vi.mocked(api.getCachedUpstreamModelCatalog).mockResolvedValue(directory);
  vi.mocked(api.fetchUpstreamModelCatalog).mockRejectedValue('UPSTREAM_MODELS_UNSUPPORTED');
  await open({}, { editing: true });
  await refresh();
  expect(field().props.options).toEqual(directory.models.map((model) => model.id));
  expect(field().props.freeModelIds).toEqual(['free-fixture']);
  expect(current.modelId).toBe('manual-fixture');
  expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'model.catalogUnsupported');
  expect(field().props.refreshing).toBe(false);
});

it('keeps the initial options when cache reading and the Add directory request both fail', async () => {
  vi.mocked(api.getCachedUpstreamModelCatalog).mockRejectedValue('unreadable cache');
  vi.mocked(api.fetchUpstreamModelCatalog).mockRejectedValue('UPSTREAM_MODELS_HTTP_503');
  await open({ modelIdOptions: ['fallback-fixture'] });
  expect(showToast).not.toHaveBeenCalled();
  await advance();
  expect(field().props.options).toEqual(['fallback-fixture']);
  expect(current.modelId).toBe('manual-fixture');
  expect(showToast).not.toHaveBeenCalled();
  await refresh();
  expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'model.catalogFailed');
});

it('does not repeat a pending refresh or leave an automatic request scheduled behind it', async () => {
  const request = deferred<api.UpstreamModelCatalog>();
  vi.mocked(api.fetchUpstreamModelCatalog).mockReturnValue(request.promise);
  await open();
  await advance(300);
  await refresh();
  expect(field().props.refreshing).toBe(true);
  await refresh();
  await advance(2000);
  expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledOnce();
  await act(async () => request.resolve(directory));
  expect(field().props.refreshing).toBe(false);
  await advance(2000);
  expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledOnce();
});

it('ignores a late old-source cache and refresh after changing to a new source', async () => {
  const cache = deferred<api.UpstreamModelCatalog | null>();
  const request = deferred<api.UpstreamModelCatalog>();
  vi.mocked(api.getCachedUpstreamModelCatalog).mockReturnValueOnce(cache.promise);
  vi.mocked(api.fetchUpstreamModelCatalog).mockReturnValueOnce(request.promise);
  await open({ modelId: '' });
  await refresh();
  await patchForm({ baseUrl: 'https://other.fixture.test/v1', apiKey: 'other-key' });
  await advance();
  expect(current.modelId).toBe('free-fixture');
  const before = edits.length;
  const stale = { models: [{ id: 'stale-model', free: true }], requiresApiKey: false };
  await act(async () => {
    cache.resolve(stale);
    request.resolve(stale);
  });
  expect(current.modelId).toBe('free-fixture');
  expect(field().props.options).toEqual(directory.models.map((model) => model.id));
  expect(edits).toHaveLength(before);
  expect(showToast).not.toHaveBeenCalled();
});

it.each(['close', 'unmount'] as const)(
  'ignores a pending request failure after %s and cancels scheduled Add work',
  async (exit) => {
    const request = deferred<api.UpstreamModelCatalog>();
    vi.mocked(api.fetchUpstreamModelCatalog).mockReturnValue(request.promise);
    await open({ modelId: '' });
    await refresh();
    await act(async () => {
      if (exit === 'close') renderer!.update(<Harness initial={initialForm()} active={false} />);
      else {
        renderer!.unmount();
        renderer = undefined;
      }
    });
    const before = edits.length;
    await act(async () => request.reject('UPSTREAM_MODELS_HTTP_503'));
    await advance(2000);
    expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledOnce();
    expect(current.modelId).toBe('');
    expect(edits).toHaveLength(before);
    expect(showToast).not.toHaveBeenCalled();
  }
);

it('closing before automatic loading cancels its timer and ignores a pending cache', async () => {
  const cache = deferred<api.UpstreamModelCatalog | null>();
  vi.mocked(api.getCachedUpstreamModelCatalog).mockReturnValue(cache.promise);
  await open({ modelId: '' });
  await advance(400);
  await act(async () => renderer!.update(<Harness initial={initialForm()} active={false} />));
  const before = edits.length;
  await act(async () => cache.resolve(directory));
  await advance(2000);
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
  expect(current.modelId).toBe('');
  expect(edits).toHaveLength(before);
  expect(field().props.disabled).toBe(true);
  expect(showToast).not.toHaveBeenCalled();
});

it('passes an encrypted API key only to catalog commands without unlocking or changing the saved form', async () => {
  await open({ apiKey: 'enc:v1:opaque-fixture' }, { editing: true });
  await advance(5000);
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
  await refresh();
  expect(api.fetchUpstreamModelCatalog).toHaveBeenCalledExactlyOnceWith({
    ...source,
    apiKey: 'enc:v1:opaque-fixture',
  });
  expect(current.apiKey).toBe('enc:v1:opaque-fixture');
  expect(api.addModel).not.toHaveBeenCalled();
  expect(api.updateModel).not.toHaveBeenCalled();
  expect(api.queryModelUsage).not.toHaveBeenCalled();
  expect(api.testModel).not.toHaveBeenCalled();
});

it('exposes refresh only for a valid HTTP endpoint and leaves manual IDs available', async () => {
  await open({ baseUrl: '', anthropicUrl: '', modelIdOptions: ['manual-option'] });
  await advance(5000);
  expect(api.getCachedUpstreamModelCatalog).not.toHaveBeenCalled();
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
  expect(field().props.onRefresh).toBeUndefined();
  expect(field().props.disabled).toBe(false);
  await act(async () => field().props.onChange('manual-only'));
  expect(current.modelId).toBe('manual-only');
  await patchForm({ baseUrl: 'file:///fixture/models.json' });
  await advance(5000);
  expect(field().props.onRefresh).toBeUndefined();
  expect(api.getCachedUpstreamModelCatalog).not.toHaveBeenCalled();
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
});

it('pastes an exact model ID and ignores clipboard responses after the modal closes', async () => {
  vi.mocked(readText).mockResolvedValueOnce('  provider/My-Custom-ID  ');
  await open({}, { editing: true });
  await act(async () => field().props.onPaste());
  expect(current.modelId).toBe('provider/My-Custom-ID');
  const clipboard = deferred<string>();
  vi.mocked(readText).mockReturnValueOnce(clipboard.promise);
  let paste!: Promise<void>;
  act(() => {
    paste = field().props.onPaste();
  });
  await act(async () => renderer!.update(<Harness initial={initialForm()} active={false} />));
  await act(async () => {
    clipboard.resolve('late-model');
    await paste;
  });
  expect(current.modelId).toBe('provider/My-Custom-ID');
  expect(api.fetchUpstreamModelCatalog).not.toHaveBeenCalled();
});

it.each(['UPSTREAM_MODELS_UNSUPPORTED', 'UPSTREAM_MODELS_HTTP_401', 'UPSTREAM_MODELS_HTTP_503'])(
  'opens Volcengine without an error toast when automatic discovery fails with %s',
  async (error) => {
    vi.mocked(api.fetchUpstreamModelCatalog).mockRejectedValue(error);
    await open({
      baseUrl: 'https://ark.cn-beijing.volces.com/api/coding/v3',
      anthropicUrl: 'https://ark.cn-beijing.volces.com/api/coding',
      apiKey: '',
      modelId: 'manual-fixture',
      modelIdOptions: ['manual-fixture'],
    });
    await advance();
    expect(showToast).not.toHaveBeenCalled();
    expect(current.modelId).toBe('manual-fixture');
    expect(field().props.options).toEqual(['manual-fixture']);
    expect(api.addModel).not.toHaveBeenCalled();
    expect(api.updateModel).not.toHaveBeenCalled();
    expect(api.queryModelUsage).not.toHaveBeenCalled();
    await refresh();
    expect(showToast).toHaveBeenCalledOnce();
  }
);
