import { act, create } from 'react-test-renderer';
import { expect, it, vi } from 'vitest';
import { Check, Copy } from 'lucide-react';
import { ChatBubble } from './ChatBubble';
const calls = vi.hoisted(() => ({ markdown: vi.fn() }));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({
    t: (key: string) => {
      const labels: Record<string, string> = {
        'mother.replyDuration': 'Took {seconds}s',
        'mother.replyTokens': '{count} tokens',
        'mother.waitThinking': 'Thinking',
        'mother.waitElapsed': 'Elapsed {duration}',
      };
      return labels[key] ?? key;
    },
    locale: 'en',
  }),
}));
vi.mock('../cards/ModelCard', () => ({ getModelIcon: () => null }));
vi.mock('../../pages/MotherAgent/mdComponents', () => ({ mdComponents: {} }));
vi.mock('react-markdown', async (original) => {
  const real = await original<typeof import('react-markdown')>();
  return {
    default: (props: React.ComponentProps<typeof real.default>) => {
      calls.markdown();
      return <real.default {...props} />;
    },
  };
});

it('copies the cleaned reply with its Markdown after streaming and displays known metrics', async () => {
  vi.useFakeTimers();
  const writeText = vi.fn(async () => {});
  vi.stubGlobal('navigator', { clipboard: { writeText } });
  vi.stubGlobal('window', { setTimeout, clearTimeout });
  const content = '<think>private reasoning</think><chat>**Visible answer**</chat>';
  const summary = { durationMs: 4000, totalTokens: 52400, estimatedCostMicros: 400 };
  let view!: ReturnType<typeof create>;
  try {
    act(() => {
      view = create(
        <ChatBubble role="assistant" content={content} replySummary={summary} isStreaming />
      );
    });
    expect(view.root.findAllByType('button')).toHaveLength(0);
    act(() =>
      view.update(<ChatBubble role="assistant" content={content} replySummary={summary} />)
    );
    expect(view.root.findByProps({ className: 'tabular-nums' }).children).toEqual([
      'Took 4s · 52.4k tokens · ~$0.0004',
    ]);
    const button = view.root.findByType('button');
    const buttonClass = button.props.className;
    expect(buttonClass).toContain('size-7');
    expect(button.findByType(Copy)).toBeDefined();
    await act(async () => await button.props.onClick());
    expect(writeText).toHaveBeenCalledExactlyOnceWith('**Visible answer**');
    expect(button.findByType(Check)).toBeDefined();
    expect(button.props['aria-label']).toBe('mother.replyCopy');
    expect(button.props.className).toBe(buttonClass);
    expect(view.root.findByProps({ role: 'status' }).children).toEqual(['mother.replyCopied']);

    act(() => {
      vi.advanceTimersByTime(1500);
    });
    await act(async () => await button.props.onClick());
    act(() => {
      vi.advanceTimersByTime(1999);
    });
    expect(button.findByType(Check)).toBeDefined();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(button.findByType(Copy)).toBeDefined();
    expect(button.props.className).toBe(buttonClass);

    act(() =>
      view.update(
        <ChatBubble role="assistant" content={content} replySummary={{ durationMs: 4000 }} />
      )
    );
    expect(view.root.findByProps({ className: 'tabular-nums' }).children).toEqual(['Took 4s']);
  } finally {
    act(() => view?.unmount());
    vi.unstubAllGlobals();
    vi.useRealTimers();
  }
});
it('keeps the round clock beside the waiting animation until answer text appears', () => {
  vi.useFakeTimers();
  vi.stubGlobal('window', { setInterval, clearInterval });
  let view!: ReturnType<typeof create>;
  try {
    const startedAt = performance.now();
    act(() => {
      view = create(<ChatBubble role="working" content="" workingStartedAt={startedAt} />);
    });
    expect(view.root.findByType('time').props.dateTime).toBe('PT0S');
    const status = view.root.findByProps({
      className: 'spinner-shimmer whitespace-nowrap tabular-nums',
    });
    expect(status.children[0]).toBe('Thinking');
    expect(status.findByType('time').children[0]).toMatch(/^Elapsed 0/);
    expect(status.findAllByProps({ 'aria-hidden': 'true' }).map((dot) => dot.children)).toEqual([
      ['·'],
      ['···'],
    ]);
    act(() => {
      vi.advanceTimersByTime(4250);
    });
    expect(view.root.findByType('time').props.dateTime).toBe('PT4S');
    expect(status.children[0]).toBe('Thinking');
    expect(status.findByType('time').children[0]).toMatch(/^Elapsed 4/);
    act(() => {
      view.update(
        <ChatBubble
          role="assistant"
          content="<think>Working"
          isStreaming
          workingStartedAt={startedAt}
        />
      );
    });
    expect(view.root.findByType('time').props.dateTime).toBe('PT4S');
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(view.root.findByType('time').props.dateTime).toBe('PT64S');
    act(() => {
      view.update(
        <ChatBubble
          role="assistant"
          content="Visible response"
          isStreaming
          workingStartedAt={startedAt}
        />
      );
    });
    expect(view.root.findAllByType('time')).toHaveLength(0);
  } finally {
    act(() => view?.unmount());
    vi.unstubAllGlobals();
    vi.useRealTimers();
  }
});

it('parses historical Markdown once while the current answer streams', () => {
  calls.markdown.mockClear();
  const transcript = (chunk: number) => (
    <>
      {Array.from({ length: 14 }, (_, index) => (
        <ChatBubble key={index} role="assistant" content={`## Saved answer ${index} **bold**`} />
      ))}
      <ChatBubble key="tail" role="assistant" content={'chunk '.repeat(chunk)} isStreaming />
    </>
  );
  let view!: ReturnType<typeof create>;
  act(() => {
    view = create(transcript(1));
  });
  for (let chunk = 2; chunk <= 21; chunk++) act(() => view.update(transcript(chunk)));
  expect(calls.markdown).toHaveBeenCalledTimes(35);
  act(() => view.unmount());
});
