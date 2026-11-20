import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { expect, it, vi } from 'vitest';
import { ToolCallCard } from './ToolCallCard';

vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: () => '执行命令', locale: 'zh-Hans' }),
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
    ).toEqual(['执行命令', '·', 'echo example']);
    act(() => {
      view!.update(<ToolCallCard name="shell_exec" args={args} status="done" output="example" />);
    });
    expect(view!.root.findAllByType('time')).toHaveLength(0);
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
