import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { expect, it, vi } from 'vitest';
import { ToolCallCard } from './ToolCallCard';

vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({
    t: (key: string) => (key === 'mother.commandUnavailable' ? '命令内容不可用' : '执行命令'),
    locale: 'zh-Hans',
  }),
}));

it('shows a compact command row with expandable arguments and output', () => {
  const args = JSON.stringify({ command: 'echo example' });
  let view: ReactTestRenderer | undefined;
  try {
    act(() => {
      view = create(<ToolCallCard name="shell_exec" args={args} status="running" />);
    });
    expect(
      view!.root
        .findByType('button')
        .findAllByType('span')
        .map((span) => span.children[0])
    ).toContain('echo example');
    expect(view!.root.findByType('button').props['aria-busy']).toBe(true);
    act(() => {
      view!.update(<ToolCallCard name="shell_exec" args={args} status="done" output="example" />);
    });
    expect(view!.root.findAllByType('time')).toHaveLength(0);
    expect(view!.root.findByType('button').props['aria-busy']).toBeUndefined();
    expect(JSON.stringify(view!.toJSON())).not.toContain(' B');
    act(() => {
      view!.root.findByType('button').props.onClick();
    });
    expect(view!.root.findByType('button').props['aria-expanded']).toBe(true);
    expect(view!.root.findAllByType('pre').map((node) => node.props.children)).toEqual([
      args,
      'example',
    ]);
  } finally {
    act(() => view?.unmount());
  }
});

it.each(['shell_exec', 'ssh_shell'])(
  'waits for readable %s content and then updates the command and byte count on each chunk',
  (name) => {
    const render = (args: string, status: 'running' | 'failed' = 'running') => (
      <ToolCallCard name={name} args={args} status={status} />
    );
    let view!: ReactTestRenderer;
    try {
      act(() => {
        view = create(render(''));
      });
      expect(view.toJSON()).toBeNull();
      for (const args of [
        '{"command":"',
        '{"meta":{"command":"wrong"}}',
        '{"meta":{"command":"nested',
      ]) {
        act(() => view.update(render(args)));
        expect(view.toJSON()).toBeNull();
      }

      const fragments = [
        ['{"server_id":"remote","command":"echo 你', 'echo 你', 41],
        ['{"server_id":"remote","command":"echo 你好 ' + '\\', 'echo 你好', 46],
        [String.raw`{"server_id":"remote","command":"echo 你好 \"🌍`, 'echo 你好 "🌍', 51],
        [String.raw`{"server_id":"remote","command":"echo 你好 \"🌍\uD83D`, 'echo 你好 "🌍', 57],
        [
          String.raw`{"server_id":"remote","command":"echo 你好 \"🌍\uD83D\uDE80`,
          'echo 你好 "🌍🚀',
          63,
        ],
      ] as const;
      for (const [args, preview, bytes] of fragments) {
        act(() => view.update(render(args)));
        const text = view.root
          .findByType('button')
          .findAllByType('span')
          .flatMap((span) => span.children.filter((child) => typeof child === 'string'));
        expect(text.slice(0, 3)).toEqual(['执行命令', '·', preview]);
        expect(text.join('')).toContain(`${bytes} B`);
      }

      const [partial, lastPreview] = fragments[fragments.length - 1];
      act(() => view.update(render(partial, 'failed')));
      const row = view.root.findByType('button');
      expect(row.props['aria-busy']).toBeUndefined();
      expect(row.findAllByType('span').some((span) => span.children.includes(lastPreview))).toBe(
        true
      );
      act(() => view.update(render('{}', 'failed')));
      expect(
        view.root
          .findByType('button')
          .findAllByType('span')
          .some((span) => span.children.includes('命令内容不可用'))
      ).toBe(true);
    } finally {
      act(() => view?.unmount());
    }
  }
);
