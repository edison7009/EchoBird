import { useCallback, useState, type ComponentProps } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ModelIdCombobox } from './ModelIdCombobox';

const { portal } = vi.hoisted(() => ({
  portal: vi.fn<(children: unknown, target: unknown) => unknown>((children) => children),
}));
vi.mock('react-dom', () => ({ createPortal: portal }));

type Props = ComponentProps<typeof ModelIdCombobox>;
let renderer: ReactTestRenderer;
let props: Props;
const blur = vi.fn();
const focus = vi.fn();
const addEventListener = vi.fn();
const removeEventListener = vi.fn();
const body = {};

function Harness(input: Props) {
  const [value, setValue] = useState(input.value);
  return (
    <ModelIdCombobox
      {...input}
      value={value}
      onChange={(next) => {
        input.onChange(next);
        setValue(next);
      }}
    />
  );
}

function ControlledHarness(input: Props) {
  const [value, setValue] = useState(input.value);
  const [open, setOpen] = useState(input.open ?? false);
  const reportOpen = input.onOpenChange;
  const onOpenChange = useCallback(
    (next: boolean) => {
      reportOpen?.(next);
      setOpen(next);
    },
    [reportOpen]
  );
  return (
    <>
      <button aria-label="Open from card" onClick={() => setOpen(true)} />
      <button aria-label="Toggle from card" onClick={() => setOpen((current) => !current)} />
      <ModelIdCombobox
        {...input}
        value={value}
        open={open}
        onOpenChange={onOpenChange}
        onChange={(next) => {
          input.onChange(next);
          setValue(next);
        }}
      />
    </>
  );
}

function mountControlled(extra: Partial<Props> = {}) {
  props = {
    value: 'one',
    options: ['one', 'two'],
    compact: true,
    selectOnly: true,
    ariaLabel: 'Route model',
    onChange: vi.fn(),
    onOpenChange: vi.fn(),
    ...extra,
  };
  act(() => {
    renderer = create(<ControlledHarness {...props} />, {
      createNodeMock: () => ({
        blur,
        focus,
        contains: () => false,
        getBoundingClientRect: () => ({ left: 12, width: 160, top: 100, bottom: 130 }),
      }),
    });
  });
}

async function updateControlled(extra: Partial<Props>) {
  props = { ...props, ...extra };
  await act(async () => renderer.update(<ControlledHarness {...props} />));
}

function mount(extra: Partial<Props> = {}) {
  props = { value: '', onChange: vi.fn(), ...extra };
  act(() => {
    renderer = create(<Harness {...props} />, {
      createNodeMock: (element) =>
        element.type === 'input'
          ? {
              blur,
              getBoundingClientRect: () => ({ left: 12, width: 240, top: 100, bottom: 130 }),
            }
          : {
              contains: () => false,
              getBoundingClientRect: () => ({ left: 10, width: 360, top: 100, bottom: 132 }),
            },
    });
  });
}

function update(extra: Partial<Props>) {
  props = { ...props, ...extra };
  act(() => renderer.update(<Harness {...props} />));
}

const input = () => renderer.root.findByType('input');
const menus = () => renderer.root.findAll((node) => node.props.style?.position === 'fixed');
const options = () => menus()[0].findAll((node) => typeof node.props.onMouseDown === 'function');
const optionTexts = () =>
  options().map((node) => node.findAllByType('span')[0]?.children[0] ?? node.children[0]);
const button = (label: string) => renderer.root.findByProps({ 'aria-label': label });
const key = (name: string) => {
  const preventDefault = vi.fn();
  act(() => input().props.onKeyDown({ key: name, preventDefault }));
  return preventDefault;
};

beforeEach(() => {
  addEventListener.mockReset();
  removeEventListener.mockReset();
  vi.stubGlobal('window', {
    innerWidth: 800,
    innerHeight: 800,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
  vi.stubGlobal('document', { body, addEventListener, removeEventListener });
});

afterEach(() => {
  act(() => renderer?.unmount());
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

it('preserves arbitrary API model input and its plain-text paste action', () => {
  const paste = vi.fn();
  mount({ onPaste: paste, pasteLabel: 'Paste model' });
  expect(input().props.readOnly).toBe(false);
  expect(input().props.style).toBeUndefined();
  act(() => input().props.onChange({ target: { value: 'custom-model-id' } }));
  expect(props.onChange).toHaveBeenCalledWith('custom-model-id');
  expect(input().props.value).toBe('custom-model-id');
  act(() => renderer.root.findByType('button').props.onClick());
  expect(paste).toHaveBeenCalledTimes(1);
  expect(portal).not.toHaveBeenCalled();

  update({ disabled: true });
  act(() => renderer.root.findByType('button').props.onClick());
  expect(paste).toHaveBeenCalledTimes(1);
});

it('preserves API browse, search, keyboard selection and the positioned body portal', () => {
  mount({ value: 'gpt-a', options: ['gpt-a', 'gpt-b', 'claude-a', 'gpt-a'] });
  act(() => input().props.onFocus());
  expect(options().map((node) => node.children[0])).toEqual(['gpt-a', 'gpt-b', 'claude-a']);
  expect(portal.mock.calls[portal.mock.calls.length - 1]?.[1]).toBe(body);
  expect(menus()[0].props.style).toMatchObject({ left: 12, width: 240, top: 131, maxHeight: 240 });
  expect(menus()[0].props.className).toContain('z-[9999]');
  expect(menus()[0].props.className).not.toContain('model-id-combobox-menu');
  expect(menus()[0].props.style.transformOrigin).toBeUndefined();
  act(() => input().props.onChange({ target: { value: 'gpt' } }));
  expect(options().map((node) => node.children[0])).toEqual(['gpt-a', 'gpt-b']);
  key('ArrowDown');
  key('ArrowDown');
  expect(key('Enter')).toHaveBeenCalledOnce();
  expect(props.onChange).toHaveBeenLastCalledWith('gpt-b');
  expect(menus()).toHaveLength(0);
  expect(blur).toHaveBeenCalledOnce();
  act(() => input().props.onClick());
  expect(options()).toHaveLength(3);
  key('Escape');
  expect(menus()).toHaveLength(0);
  act(() => input().props.onClick());
  const outside = addEventListener.mock.calls.find(([name]) => name === 'mousedown')?.[1];
  act(() => outside({ target: {} }));
  expect(menus()).toHaveLength(0);
});

it('lays out the API hint, refresh and paste beside free input without overlaying long IDs', () => {
  const refresh = vi.fn();
  const paste = vi.fn();
  const id = 'provider/a-long-model-identifier-with-a-release-and-version';
  mount({
    value: id,
    inlineActions: true,
    actionHint: '需要先填入密钥',
    onRefresh: refresh,
    refreshLabel: 'Read models',
    onPaste: paste,
    pasteLabel: 'Paste model',
  });
  expect(input().props.readOnly).toBe(false);
  expect(input().props.value).toBe(id);
  expect(input().props.className).toContain('min-w-0 flex-1');
  expect(input().props.className).not.toMatch(/pr-\d/);
  expect(input().parent?.props.className).toContain('flex items-center');
  const actions = input().parent!.findByProps({
    className: 'flex items-center gap-2 shrink-0 pr-2',
  });
  expect(actions.findByType('span').children).toEqual(['需要先填入密钥']);
  expect(
    actions.findAllByType('button').map((node) => node.props['aria-label'] ?? node.children[0])
  ).toEqual(['Read models', 'Paste model']);
  expect(
    actions.findAllByType('button').every((node) => !node.props.className.includes('absolute'))
  ).toBe(true);
  const preventDefault = vi.fn();
  const stopPropagation = vi.fn();
  act(() => {
    button('Read models').props.onMouseDown({ preventDefault });
    button('Read models').props.onClick({ stopPropagation });
    actions.findAllByType('button')[1].props.onMouseDown({ preventDefault });
    actions.findAllByType('button')[1].props.onClick({ stopPropagation });
  });
  expect(refresh).toHaveBeenCalledOnce();
  expect(paste).toHaveBeenCalledOnce();
  expect(preventDefault).toHaveBeenCalledTimes(2);
  expect(stopPropagation).toHaveBeenCalledTimes(2);
  expect(props.onChange).not.toHaveBeenCalled();
  expect(menus()).toHaveLength(0);
  act(() => input().props.onChange({ target: { value: 'arbitrary-api-id' } }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('arbitrary-api-id');
});

it('retains paste beside suggestions, removes the API hint and positions the menu across the whole group', () => {
  mount({
    value: 'saved-id',
    options: ['saved-id', 'other-id'],
    inlineActions: true,
    actionHint: '需要先填入密钥',
    onRefresh: vi.fn(),
    refreshLabel: 'Read models',
    onPaste: vi.fn(),
    pasteLabel: 'Paste model',
  });
  expect(
    renderer.root
      .findAllByType('button')
      .map((node) => node.props['aria-label'] ?? node.children[0])
  ).toEqual(['Read models', 'Paste model', 'Toggle model id suggestions']);
  update({ actionHint: undefined });
  expect(JSON.stringify(renderer.toJSON())).not.toContain('需要先填入密钥');
  act(() => input().props.onFocus());
  expect(menus()[0].props.style).toMatchObject({ left: 10, width: 360, top: 133 });
  act(() => button('Toggle model id suggestions').props.onClick({ stopPropagation: vi.fn() }));
  expect(menus()).toHaveLength(0);
  key('ArrowDown');
  expect(menus()).toHaveLength(1);
  key('ArrowDown');
  key('ArrowDown');
  key('Enter');
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('other-id');
  expect(input().props.value).toBe('other-id');
  expect(menus()).toHaveLength(0);
});

it('isolates inline API actions from an open menu and leaves typing and selection usable while refreshing', () => {
  const refresh = vi.fn();
  const paste = vi.fn();
  mount({
    value: 'saved-id',
    options: ['saved-id', 'other-id'],
    inlineActions: true,
    onRefresh: refresh,
    refreshLabel: 'Read models',
    onPaste: paste,
    pasteLabel: 'Paste model',
  });
  act(() => input().props.onFocus());
  const menu = menus()[0];
  act(() => {
    button('Read models').props.onClick({ stopPropagation: vi.fn() });
    renderer.root.findAllByType('button')[1].props.onClick({ stopPropagation: vi.fn() });
  });
  expect(refresh).toHaveBeenCalledOnce();
  expect(paste).toHaveBeenCalledOnce();
  expect(menus()[0]).toBe(menu);
  expect(props.onChange).not.toHaveBeenCalled();
  update({ refreshing: true });
  expect(button('Read models').props.disabled).toBe(true);
  expect(input().props.disabled).toBe(false);
  expect(button('Toggle model id suggestions').props.disabled).toBe(false);
  act(() => button('Read models').props.onClick({ stopPropagation: vi.fn() }));
  expect(refresh).toHaveBeenCalledOnce();
  act(() => options()[1].props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('other-id');
  act(() => input().props.onChange({ target: { value: 'manual-id' } }));
  expect(props.onChange).toHaveBeenLastCalledWith('manual-id');
});

it('shows reliable positive multipliers without rounding and prioritizes the free label', () => {
  mount({
    value: 'paid-id',
    options: [
      'paid-id',
      'free-id',
      'precise-id',
      'zero-id',
      'negative-id',
      'invalid-id',
      'infinite-id',
      'unknown-id',
    ],
    optionLabels: { 'paid-id': 'Paid display name' },
    optionMultipliers: {
      'paid-id': 0.79,
      'free-id': 1,
      'precise-id': 1.23456789,
      'zero-id': 0,
      'negative-id': -1,
      'invalid-id': NaN,
      'infinite-id': Infinity,
    },
    freeModelIds: ['free-id'],
    freeLabel: '免费',
  });
  expect(input().props.value).toBe('Paid display name');
  act(() => input().props.onFocus());
  const badges = (index: number) => options()[index].findAllByType('span');
  expect(badges(0).map((node) => node.children)).toEqual([['free-id'], ['免费']]);
  expect(badges(0)[1].props.className).toContain('text-cyber-accent');
  expect(badges(1).map((node) => node.children)).toEqual([['zero-id'], ['免费']]);
  expect(badges(2).map((node) => node.children)).toEqual([['Paid display name'], ['0.79×']]);
  expect(badges(2)[1].props.className).toContain('text-cyber-text-muted');
  expect(badges(3).map((node) => node.children)).toEqual([['precise-id'], ['1.23456789×']]);
  for (let index = 4; index < options().length; index += 1) expect(badges(index)).toHaveLength(0);
  act(() => options()[2].props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('paid-id');
  expect(input().props.value).toBe('Paid display name');
  expect(menus()).toHaveLength(0);
  act(() => input().props.onChange({ target: { value: '0.79×' } }));
  expect(menus()).toHaveLength(0);
});

it('opens controlled compact choices from the card, focuses the trigger, and closes once on selection', () => {
  mountControlled();
  expect(menus()).toHaveLength(0);
  expect(focus).not.toHaveBeenCalled();
  act(() => button('Open from card').props.onClick());
  expect(options().map((node) => node.children[0])).toEqual(['one', 'two']);
  expect(button('Route model').props['aria-expanded']).toBe(true);
  expect(focus).toHaveBeenCalledOnce();
  expect(props.onOpenChange).not.toHaveBeenCalled();
  const openMenu = menus()[0];
  expect(openMenu.props.className).toContain('model-id-combobox-menu');
  expect(openMenu.props.className).not.toContain('is-drop-up');
  expect(openMenu.props.style.transformOrigin).toBe('top');
  expect(openMenu.props.style.maxHeight).toBe(360);
  act(() => button('Open from card').props.onClick());
  expect(menus()[0]).toBe(openMenu);
  expect(focus).toHaveBeenCalledOnce();
  act(() => options()[1].props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('two');
  expect(props.onOpenChange).toHaveBeenCalledExactlyOnceWith(false);
  expect(blur).toHaveBeenCalledOnce();
  expect(menus()).toHaveLength(0);
  expect(button('Route model').props['aria-expanded']).toBe(false);
  act(() => button('Open from card').props.onClick());
  expect(focus).toHaveBeenCalledTimes(2);
  expect(button('Route model').findByType('span').children).toEqual(['two']);
});

it.each([1, 2])(
  'ignores mouse button %i on compact model options without selecting or closing',
  (mouseButton) => {
    mountControlled();
    act(() => button('Open from card').props.onClick());
    const preventDefault = vi.fn();
    act(() => options()[1].props.onMouseDown({ button: mouseButton, preventDefault }));
    expect(props.onChange).not.toHaveBeenCalled();
    expect(props.onOpenChange).not.toHaveBeenCalled();
    expect(preventDefault).not.toHaveBeenCalled();
    expect(button('Route model').props['aria-expanded']).toBe(true);
    expect(options()).toHaveLength(2);
    act(() => options()[1].props.onMouseDown({ button: 0, preventDefault }));
    expect(props.onChange).toHaveBeenCalledExactlyOnceWith('two');
    expect(props.onOpenChange).toHaveBeenCalledExactlyOnceWith(false);
    expect(menus()).toHaveLength(0);
    act(() => button('Open from card').props.onClick());
    act(() => button('Route model').props.onKeyDown({ key: 'ArrowDown', preventDefault }));
    act(() => button('Route model').props.onKeyDown({ key: 'Enter', preventDefault }));
    expect(props.onChange).toHaveBeenCalledTimes(2);
    expect(props.onChange).toHaveBeenLastCalledWith('one');
    expect(props.onOpenChange).toHaveBeenCalledTimes(2);
    expect(menus()).toHaveLength(0);
  }
);

it('preserves external card and logo mousedown until click toggles the menu, while real outside clicks close it', () => {
  const cardTarget = {} as Node;
  const logoTarget = {} as Node;
  const outsideTarget = {} as Node;
  const listeners = new Set<(event: { target: Node }) => void>();
  addEventListener.mockImplementation((name, listener) => {
    if (name === 'mousedown') listeners.add(listener);
  });
  removeEventListener.mockImplementation((name, listener) => {
    if (name === 'mousedown') listeners.delete(listener);
  });
  mountControlled({
    externalTriggerRef: {
      current: {
        contains: (target: Node) => target === cardTarget || target === logoTarget,
      } as HTMLElement,
    },
  });
  const mouseDown = (target: Node) => {
    act(() => listeners.forEach((listener) => listener({ target })));
  };
  const clickCard = () => act(() => button('Toggle from card').props.onClick());

  mouseDown(cardTarget);
  clickCard();
  expect(menus()).toHaveLength(1);
  const firstMenu = menus()[0];
  mouseDown(logoTarget);
  expect(menus()[0]).toBe(firstMenu);
  expect(props.onOpenChange).not.toHaveBeenCalled();
  clickCard();
  expect(menus()).toHaveLength(0);
  expect(listeners.size).toBe(0);

  mouseDown(logoTarget);
  clickCard();
  expect(menus()).toHaveLength(1);
  expect(menus()[0]).not.toBe(firstMenu);
  mouseDown(cardTarget);
  expect(menus()).toHaveLength(1);
  clickCard();
  expect(menus()).toHaveLength(0);

  clickCard();
  mouseDown(outsideTarget);
  expect(menus()).toHaveLength(0);
  expect(props.onOpenChange).toHaveBeenCalledExactlyOnceWith(false);
  expect(props.onChange).not.toHaveBeenCalled();
});

it('clears a hovered option when card mousedown and click close then reopen, while fresh keyboard choices still work', async () => {
  const cardTarget = {} as Node;
  const listeners = new Set<(event: { target: Node }) => void>();
  addEventListener.mockImplementation((name, listener) => {
    if (name === 'mousedown') listeners.add(listener);
  });
  removeEventListener.mockImplementation((name, listener) => {
    if (name === 'mousedown') listeners.delete(listener);
  });
  mountControlled({
    externalTriggerRef: {
      current: { contains: (target: Node) => target === cardTarget } as HTMLElement,
    },
  });
  const clickCard = async () => {
    act(() => listeners.forEach((listener) => listener({ target: cardTarget })));
    await act(async () => button('Toggle from card').props.onClick());
  };
  const press = (key: string) => {
    act(() => button('Route model').props.onKeyDown({ key, preventDefault: vi.fn() }));
  };

  await clickCard();
  act(() => options()[1].props.onMouseEnter());
  expect(options()[1].props.className.split(' ')).toContain('bg-cyber-text/10');
  await clickCard();
  expect(menus()).toHaveLength(0);
  await clickCard();
  expect(options()[1].props.className.split(' ')).not.toContain('bg-cyber-text/10');
  press('Enter');
  expect(props.onChange).not.toHaveBeenCalled();
  press('ArrowDown');
  press('Enter');
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('one');
  expect(menus()).toHaveLength(0);

  await act(async () => button('Route model').props.onClick());
  press('ArrowDown');
  press('ArrowDown');
  press('Enter');
  expect(vi.mocked(props.onChange).mock.calls.map(([id]) => id)).toEqual(['one', 'two']);
  expect(menus()).toHaveLength(0);
});

it('reports one controlled transition for trigger toggles, Escape and outside clicks', () => {
  mountControlled();
  const trigger = () => button('Route model');
  act(() => trigger().props.onClick());
  expect(props.onOpenChange).toHaveBeenCalledExactlyOnceWith(true);
  expect(menus()).toHaveLength(1);
  act(() => trigger().props.onClick());
  expect(vi.mocked(props.onOpenChange!).mock.calls.map(([open]) => open)).toEqual([true, false]);
  expect(menus()).toHaveLength(0);
  vi.mocked(props.onOpenChange!).mockClear();

  act(() => button('Open from card').props.onClick());
  act(() => trigger().props.onKeyDown({ key: 'Escape', preventDefault: vi.fn() }));
  expect(props.onOpenChange).toHaveBeenCalledExactlyOnceWith(false);
  expect(menus()).toHaveLength(0);
  vi.mocked(props.onOpenChange!).mockClear();

  act(() => button('Open from card').props.onClick());
  const outsideListeners = addEventListener.mock.calls.filter(([name]) => name === 'mousedown');
  const outside = outsideListeners[outsideListeners.length - 1]?.[1];
  act(() => outside({ target: {} }));
  expect(props.onOpenChange).toHaveBeenCalledExactlyOnceWith(false);
  expect(menus()).toHaveLength(0);
  expect(removeEventListener).toHaveBeenCalledWith('mousedown', outside);
  expect(props.onChange).not.toHaveBeenCalled();
});

it('resets controlled open state while disabled and stays closed after enabling', async () => {
  mountControlled({ disabled: true });
  await act(async () => button('Open from card').props.onClick());
  expect(menus()).toHaveLength(0);
  expect(focus).not.toHaveBeenCalled();
  expect(props.onOpenChange).toHaveBeenCalledExactlyOnceWith(false);
  act(() => button('Route model').props.onClick());
  expect(props.onOpenChange).toHaveBeenCalledOnce();
  await updateControlled({ disabled: false });
  expect(menus()).toHaveLength(0);
  expect(focus).not.toHaveBeenCalled();
  act(() => button('Open from card').props.onClick());
  expect(menus()).toHaveLength(1);
  expect(focus).toHaveBeenCalledOnce();
  await updateControlled({ disabled: true });
  expect(menus()).toHaveLength(0);
  expect(props.onOpenChange).toHaveBeenCalledTimes(2);
  await updateControlled({ disabled: false });
  expect(menus()).toHaveLength(0);
});

it('rejects controlled opens without options and does not open when options later load', async () => {
  mountControlled({ options: [] });
  await act(async () => button('Open from card').props.onClick());
  expect(menus()).toHaveLength(0);
  expect(focus).not.toHaveBeenCalled();
  expect(props.onOpenChange).toHaveBeenCalledExactlyOnceWith(false);
  expect(button('Route model').props.disabled).toBe(true);
  await updateControlled({ options: ['one', 'two'] });
  expect(menus()).toHaveLength(0);
  expect(focus).not.toHaveBeenCalled();
  act(() => button('Open from card').props.onClick());
  expect(menus()).toHaveLength(1);
  expect(focus).toHaveBeenCalledOnce();
  expect(props.onChange).not.toHaveBeenCalled();
});

it('keeps all cached compact choices through display-label updates and viewport resize, then selects the current ID', async () => {
  const ids = Array.from({ length: 40 }, (_, index) => `model-${index}`);
  mountControlled({ value: ids[0], options: ids, optionLabels: { [ids[39]]: 'Short name' } });
  act(() => button('Open from card').props.onClick());
  const menu = menus()[0];
  expect(options()).toHaveLength(40);
  expect(menu.props.style.maxHeight).toBe(360);
  expect(menu.props.className).toContain('overflow-y-auto');
  expect(options()[39].children).toEqual(['Short name']);

  const longLabel = 'provider/a-long-display-label-with-a-version-and-release-identifier';
  await updateControlled({ optionLabels: { [ids[39]]: longLabel } });
  expect(menus()[0]).toBe(menu);
  expect(options()[39].children).toEqual([longLabel]);
  expect(menus()[0].props.style.width).toBe('max-content');
  expect(props.onChange).not.toHaveBeenCalled();

  const newId = 'opaque-updated-id';
  await updateControlled({
    options: [...ids.slice(0, 39), newId],
    optionLabels: { [newId]: longLabel },
  });
  expect(options()).toHaveLength(40);
  expect(menus()[0]).toBe(menu);
  expect(options()[39].children).toEqual([longLabel]);
  const resize = vi
    .mocked(window.addEventListener)
    .mock.calls.find(([name]) => name === 'resize')?.[1] as (() => void) | undefined;
  expect(resize).toBeTypeOf('function');
  Object.assign(window, { innerWidth: 200, innerHeight: 220 });
  act(() => resize?.());
  expect(menus()[0]).toBe(menu);
  expect(menu.props.style.top).toBeUndefined();
  expect(menu.props.style.bottom).toBe(121);
  expect(menu.props.style.maxHeight).toBe(91);
  expect(menu.props.style.bottom + menu.props.style.maxHeight).toBe(212);
  expect(menu.props.style.maxWidth).toBe('calc(100vw - 16px)');
  expect(options()).toHaveLength(40);

  Object.assign(window, { innerWidth: 800, innerHeight: 800 });
  act(() => resize?.());
  expect(menu.props.style.top).toBe(131);
  expect(menu.props.style.maxHeight).toBe(360);
  expect(menu.props.style.bottom).toBeUndefined();
  expect(props.onChange).not.toHaveBeenCalled();
  act(() => options()[39].props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith(newId);
  expect(props.onOpenChange).toHaveBeenCalledExactlyOnceWith(false);
  expect(menus()).toHaveLength(0);
});

it('shows labels while select-only choices preserve distinct IDs with duplicate labels', () => {
  const paste = vi.fn();
  mount({
    value: 'first-id',
    options: ['first-id', 'second-id', 'unlabelled-id'],
    optionLabels: { 'first-id': 'Same account', 'second-id': 'Same account' },
    selectOnly: true,
    ariaLabel: 'Account',
    onPaste: paste,
    pasteLabel: 'Paste account',
  });
  expect(input().props.readOnly).toBe(true);
  expect(input().props.style).toEqual({ cursor: 'default' });
  expect(input().props['aria-label']).toBe('Account');
  expect(input().props.value).toBe('Same account');
  act(() => input().props.onChange({ target: { value: 'typed account' } }));
  expect(props.onChange).not.toHaveBeenCalled();
  expect(renderer.root.findAllByType('button')).toHaveLength(1);
  act(() => input().props.onFocus());
  expect(options().map((node) => node.children[0])).toEqual([
    'Same account',
    'Same account',
    'unlabelled-id',
  ]);
  const preventDefault = vi.fn();
  act(() => options()[1].props.onMouseDown({ button: 0, preventDefault }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('second-id');
  expect(preventDefault).toHaveBeenCalledOnce();
  expect(paste).not.toHaveBeenCalled();
});

it('searches display labels and still commits the selected ID', () => {
  mount({
    options: ['opaque-one', 'opaque-two'],
    optionLabels: { 'opaque-one': 'Claude account', 'opaque-two': 'Codex account' },
  });
  act(() => input().props.onChange({ target: { value: 'codex' } }));
  expect(options().map((node) => node.children[0])).toEqual(['Codex account']);
  key('ArrowDown');
  key('Enter');
  expect(props.onChange).toHaveBeenLastCalledWith('opaque-two');
  expect(input().props.value).toBe('Codex account');
});

it('marks supplied free metadata while preserving plain option and selected labels', () => {
  mount({
    value: 'opaque-free',
    options: ['opaque-free', 'paid-model', 'unknown-model'],
    optionLabels: { 'opaque-free': 'Hy3' },
    freeModelIds: ['opaque-free', 'absent-model'],
    freeLabel: '免费',
  });
  expect(input().props.value).toBe('Hy3');
  act(() => input().props.onFocus());
  expect(
    options()[0]
      .findAllByType('span')
      .map((node) => node.children)
  ).toEqual([['Hy3'], ['免费']]);
  expect(options()[1].children).toEqual(['paid-model']);
  expect(options()[2].children).toEqual(['unknown-model']);
  expect(options()).toHaveLength(3);
  expect(props.onChange).not.toHaveBeenCalled();
});

it('stably puts free models first without changing a manually entered ID', () => {
  mount({
    value: 'manual/Unlisted-ID',
    options: [
      'paid-a',
      'provider/free-model',
      'zero-ratio',
      'paid-b',
      'carefree',
      'opaque',
      'provider/free-model',
      'foo:free',
      'zero-blocked',
      'big-pickle',
    ],
    freeModelIds: ['opaque', 'big-pickle'],
    paidModelIds: ['zero-blocked'],
    optionMultipliers: { 'zero-ratio': 0, 'foo:free': 2, 'zero-blocked': 0, opaque: 1 },
    freeLabel: '免费',
  });
  act(() => input().props.onFocus());
  expect(optionTexts()).toEqual([
    'provider/free-model',
    'zero-ratio',
    'carefree',
    'opaque',
    'big-pickle',
    'paid-a',
    'paid-b',
    'foo:free',
    'zero-blocked',
  ]);
  for (const row of options().slice(0, 5)) {
    const badge = row.findAllByType('span')[1];
    expect(badge.children).toEqual(['免费']);
    expect(badge.props.className).toContain('text-cyber-accent');
  }
  expect(input().props.value).toBe('manual/Unlisted-ID');
  expect(props.onChange).not.toHaveBeenCalled();
});

it.each([
  ['FREE', true],
  ['provider/model:free', true],
  ['model-free', true],
  ['model_free', true],
  ['model.free', true],
  ['free/v1', true],
  ['provider/model:FrEeTier', true],
  ['tierFREE', true],
  ['freedom', true],
  ['freestyle', true],
  ['carefree', true],
  ['freezer', true],
  ['model-freeform', true],
  ['free2', true],
  ['2free', true],
  ['big-pickle', false],
])(
  'recognizes free anywhere in %s regardless of case without guessing unrelated names',
  (id, free) => {
    mount({ options: [id as string], freeLabel: '免费' });
    act(() => input().props.onFocus());
    expect(
      options()[0]
        .findAllByType('span')
        .some((node) => node.children[0] === '免费')
    ).toBe(free);
  }
);

it('lets explicit paid metadata override the free name and updates without selecting', () => {
  mount({ value: 'model-free', options: ['paid-model', 'model-free'] });
  act(() => input().props.onFocus());
  expect(optionTexts()).toEqual(['model-free', 'paid-model']);
  update({ paidModelIds: ['model-free'] });
  expect(optionTexts()).toEqual(['paid-model', 'model-free']);
  expect(options()[1].findAllByType('span')).toHaveLength(0);
  expect(input().props.value).toBe('model-free');
  expect(props.onChange).not.toHaveBeenCalled();
});

it('keeps non-model IDs in source order when name inference is disabled but honors metadata', () => {
  mount({
    value: 'free-account',
    options: ['paid-provider', 'free-account', 'free-provider'],
    inferFreeFromId: false,
    freeLabel: '免费',
  });
  act(() => input().props.onFocus());
  expect(optionTexts()).toEqual(['paid-provider', 'free-account', 'free-provider']);
  for (const row of options()) expect(row.findAllByType('span')).toHaveLength(0);
  update({
    freeModelIds: ['free-provider'],
    optionMultipliers: { 'paid-provider': 0 },
  });
  expect(optionTexts()).toEqual(['paid-provider', 'free-provider', 'free-account']);
  expect(options()[0].findAllByType('span')[1].children).toEqual(['免费']);
  expect(options()[1].findAllByType('span')[1].children).toEqual(['免费']);
  expect(options()[2].findAllByType('span')).toHaveLength(0);
  expect(input().props.value).toBe('free-account');
  expect(props.onChange).not.toHaveBeenCalled();
});

it('uses the sorted compact options for keyboard selection while returning the original ID', () => {
  mountControlled({ value: 'paid-model', options: ['paid-model', 'provider/Model:Free'] });
  act(() => button('Open from card').props.onClick());
  expect(optionTexts()).toEqual(['provider/Model:Free', 'paid-model']);
  act(() => button('Route model').props.onKeyDown({ key: 'ArrowDown', preventDefault: vi.fn() }));
  act(() => button('Route model').props.onKeyDown({ key: 'Enter', preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('provider/Model:Free');
  expect(button('Route model').findByType('span').children).toEqual(['provider/Model:Free']);
});

it('selects a free labelled choice by mouse using its original ID', () => {
  mount({
    options: ['opaque-free', 'paid-model'],
    optionLabels: { 'opaque-free': 'Hy3' },
    freeModelIds: ['opaque-free'],
    freeLabel: '免费',
  });
  act(() => input().props.onFocus());
  act(() => options()[0].props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('opaque-free');
  expect(input().props.value).toBe('Hy3');
  expect(menus()).toHaveLength(0);
});

it('searches model labels without treating the free badge as searchable model text', () => {
  mount({
    options: ['opaque-free', 'paid-model'],
    optionLabels: { 'opaque-free': 'Hy3' },
    freeModelIds: ['opaque-free'],
    freeLabel: '免费',
  });
  act(() => input().props.onChange({ target: { value: '免费' } }));
  expect(menus()).toHaveLength(0);
  act(() => input().props.onChange({ target: { value: 'hy3' } }));
  expect(options()).toHaveLength(1);
  key('ArrowDown');
  key('Enter');
  expect(props.onChange).toHaveBeenLastCalledWith('opaque-free');
  expect(input().props.value).toBe('Hy3');
});

it('keeps compact free choices on their original IDs through keyboard selection and disabling', async () => {
  mountControlled({
    value: 'paid-model',
    options: ['paid-model', 'opaque-free'],
    optionLabels: { 'opaque-free': 'Hy3' },
    freeModelIds: ['opaque-free'],
    freeLabel: '免费',
  });
  act(() => button('Open from card').props.onClick());
  act(() => button('Route model').props.onKeyDown({ key: 'ArrowDown', preventDefault: vi.fn() }));
  act(() => button('Route model').props.onKeyDown({ key: 'Enter', preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('opaque-free');
  expect(button('Route model').findByType('span').children).toEqual(['Hy3']);
  expect(menus()).toHaveLength(0);
  act(() => button('Open from card').props.onClick());
  await updateControlled({ disabled: true });
  expect(menus()).toHaveLength(0);
  act(() => button('Route model').props.onClick());
  act(() => button('Route model').props.onKeyDown({ key: 'Enter', preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenCalledOnce();
});

it('allows select-only keyboard selection but disables the chevron without options', () => {
  mount({ options: ['one', 'two'], selectOnly: true });
  key('ArrowDown');
  key('ArrowDown');
  key('ArrowDown');
  key('Enter');
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('two');
  update({ options: [], placeholder: '—' });
  const chevron = button('Toggle model id suggestions');
  expect(chevron.props.disabled).toBe(true);
  expect(input().props.placeholder).toBe('—');
  act(() => {
    input().props.onFocus();
    input().props.onClick();
    chevron.props.onClick();
  });
  key('ArrowDown');
  expect(menus()).toHaveLength(0);
  expect(props.onChange).toHaveBeenCalledOnce();
});

it('refreshes independently of menu state and selection, with a separate busy lock', () => {
  const refresh = vi.fn();
  mount({
    value: 'one',
    options: ['one', 'two'],
    selectOnly: true,
    onRefresh: refresh,
    refreshLabel: 'Read models',
  });
  const preventDefault = vi.fn();
  const stopPropagation = vi.fn();
  act(() => {
    button('Read models').props.onMouseDown({ preventDefault });
    button('Read models').props.onClick({ stopPropagation });
  });
  expect(refresh).toHaveBeenCalledOnce();
  expect(preventDefault).toHaveBeenCalledOnce();
  expect(stopPropagation).toHaveBeenCalledOnce();
  expect(menus()).toHaveLength(0);
  expect(props.onChange).not.toHaveBeenCalled();
  expect(button('Read models').props.className).toContain('right-7');
  expect(button('Toggle model id suggestions').props.className).toContain('right-1.5');
  expect(input().props.className).toContain('pr-14');
  act(() => input().props.onFocus());
  act(() => button('Read models').props.onClick({ stopPropagation }));
  expect(refresh).toHaveBeenCalledTimes(2);
  expect(options()).toHaveLength(2);
  expect(input().props.value).toBe('one');
  update({ refreshing: true });
  expect(button('Read models').props.disabled).toBe(true);
  act(() => button('Read models').props.onClick({ stopPropagation }));
  expect(refresh).toHaveBeenCalledTimes(2);
  expect(options()).toHaveLength(2);
});

it('closes an open disabled field and prevents all selection or refresh callbacks', async () => {
  const refresh = vi.fn();
  mount({ options: ['one'], onRefresh: refresh, refreshLabel: 'Read models' });
  act(() => input().props.onFocus());
  expect(menus()).toHaveLength(1);
  await act(async () => update({ disabled: true }));
  expect(input().props.disabled).toBe(true);
  expect(menus()).toHaveLength(0);
  act(() => {
    input().props.onChange({ target: { value: 'typed' } });
    input().props.onFocus();
    input().props.onClick();
    button('Toggle model id suggestions').props.onClick();
    button('Read models').props.onClick({ stopPropagation: vi.fn() });
  });
  key('ArrowDown');
  key('Enter');
  expect(props.onChange).not.toHaveBeenCalled();
  expect(refresh).not.toHaveBeenCalled();
  update({ disabled: false });
  expect(menus()).toHaveLength(0);
});

it('does not commit an out-of-range option after the directory changes', () => {
  mount({ options: ['one', 'two'], selectOnly: true });
  act(() => input().props.onFocus());
  key('ArrowDown');
  key('ArrowDown');
  update({ options: ['one'] });
  key('Enter');
  expect(props.onChange).not.toHaveBeenCalled();
  key('ArrowUp');
  key('Enter');
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('one');
});

it('keeps compact route choices usable for short and long IDs, within the viewport, and through keyboard and disabled states', async () => {
  const shortId = 'model-a';
  const longId = 'provider/model-with-a-long-version-and-release-identifier';
  props = {
    value: shortId,
    options: [shortId, longId],
    onChange: vi.fn(),
    compact: true,
    selectOnly: true,
    ariaLabel: 'Route model',
  };
  vi.stubGlobal('window', {
    innerWidth: 800,
    innerHeight: 800,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
  const triggerBounds = { left: 740, width: 52, top: 740, bottom: 764 };
  act(() => {
    renderer = create(<Harness {...props} />, {
      createNodeMock: () => ({
        blur,
        contains: () => false,
        getBoundingClientRect: () => triggerBounds,
      }),
    });
  });
  const trigger = () => button('Route model');
  const press = (name: string) => {
    const preventDefault = vi.fn();
    act(() => trigger().props.onKeyDown({ key: name, preventDefault }));
    return preventDefault;
  };
  expect(JSON.stringify(renderer.toJSON())).toContain(shortId);
  act(() => trigger().props.onClick());
  expect(options().map((node) => node.children[0])).toEqual([shortId, longId]);
  expect(portal.mock.calls[portal.mock.calls.length - 1]?.[1]).toBe(body);
  const menuPosition = menus()[0].props.style;
  expect(menuPosition.left).toBeGreaterThanOrEqual(8);
  expect(menuPosition.width).toBe('max-content');
  expect(menuPosition.minWidth).toBe('min(52px, calc(100vw - 16px))');
  expect(menuPosition.maxWidth).toBe('calc(100vw - 16px)');
  expect(menuPosition['--model-menu-left']).toBe('740px');
  expect(menuPosition.top).toBeUndefined();
  expect(menuPosition.transformOrigin).toBe('bottom');
  expect(menus()[0].props.className).toContain('is-drop-up');
  expect(menuPosition.bottom).toBeGreaterThan(0);
  expect(menuPosition.bottom + menuPosition.maxHeight).toBeLessThanOrEqual(800);
  const resize = vi
    .mocked(window.addEventListener)
    .mock.calls.find(([name]) => name === 'resize')?.[1] as (() => void) | undefined;
  Object.assign(window, { innerWidth: 120, innerHeight: 120 });
  Object.assign(triggerBounds, { left: 4, top: 40, bottom: 64 });
  act(() => resize?.());
  expect(menus()[0].props.style.left).toBe(8);
  expect(menus()[0].props.style.maxHeight).toBe(47);
  expect(menus()[0].props.style.top + menus()[0].props.style.maxHeight).toBe(112);
  expect(options()).toHaveLength(2);
  Object.assign(window, { innerWidth: 800, innerHeight: 800 });
  Object.assign(triggerBounds, { left: 740, top: 740, bottom: 764 });
  act(() => resize?.());
  const preventDefault = vi.fn();
  act(() => options()[1].props.onMouseDown({ button: 0, preventDefault }));
  expect(preventDefault).toHaveBeenCalledOnce();
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith(longId);
  expect(JSON.stringify(renderer.toJSON())).toContain(longId);
  expect(menus()).toHaveLength(0);
  press('ArrowDown');
  expect(options().map((node) => node.children[0])).toEqual([shortId, longId]);
  press('ArrowDown');
  expect(press('Enter')).toHaveBeenCalledOnce();
  expect(vi.mocked(props.onChange).mock.calls.map(([id]) => id)).toEqual([longId, shortId]);
  expect(menus()).toHaveLength(0);
  act(() => trigger().props.onClick());
  press('Escape');
  expect(menus()).toHaveLength(0);
  act(() => trigger().props.onClick());
  expect(menus()).toHaveLength(1);
  await act(async () => update({ disabled: true }));
  expect(trigger().props.disabled).toBe(true);
  expect(menus()).toHaveLength(0);
  act(() => trigger().props.onClick());
  press('ArrowDown');
  press('Enter');
  expect(menus()).toHaveLength(0);
  expect(props.onChange).toHaveBeenCalledTimes(2);
  update({ disabled: false });
  expect(menus()).toHaveLength(0);
});

it('retains the free badge after selection and clears it when switching to a paid model', () => {
  mountControlled({
    value: 'paid-model',
    options: ['hy3', 'paid-model'],
    freeModelIds: ['hy3'],
    freeLabel: '免费',
    showSelectedFreeLabel: true,
  });
  const badge = () =>
    button('Route model').findAllByProps({
      className: 'shrink-0 font-sans text-cyber-accent',
    });
  expect(badge()).toHaveLength(0);
  act(() => button('Open from card').props.onClick());
  act(() => options()[0].props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenCalledExactlyOnceWith('hy3');
  expect(badge()[0].children).toEqual(['免费']);
  expect(menus()).toHaveLength(0);
  act(() => button('Open from card').props.onClick());
  act(() => options()[1].props.onMouseDown({ button: 0, preventDefault: vi.fn() }));
  expect(props.onChange).toHaveBeenLastCalledWith('paid-model');
  expect(badge()).toHaveLength(0);
});
