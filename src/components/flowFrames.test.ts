import { afterEach, expect, it, vi } from 'vitest';
import { requestFlowFrame, cancelFlowFrame } from './flowFrames';

afterEach(() => vi.unstubAllGlobals());

it('batches multiple controls in one native frame and cancels owners independently', () => {
  const callbacks = new Map<number, FrameRequestCallback>();
  let serial = 0;
  const request = vi.fn((callback: FrameRequestCallback) => {
    callbacks.set(++serial, callback);
    return serial;
  });
  vi.stubGlobal('requestAnimationFrame', request);
  vi.stubGlobal('cancelAnimationFrame', (id: number) => callbacks.delete(id));
  const tab = vi.fn(),
    pill = vi.fn();
  const a = requestFlowFrame(tab),
    b = requestFlowFrame(pill);
  expect(request).toHaveBeenCalledTimes(1);
  cancelFlowFrame(a);
  const native = [...callbacks.values()][0];
  callbacks.clear();
  native(123);
  expect(tab).not.toHaveBeenCalled();
  expect(pill).toHaveBeenCalledWith(123);
  const c = requestFlowFrame(tab),
    d = requestFlowFrame(pill);
  expect(callbacks.size).toBe(1);
  cancelFlowFrame(c);
  expect(callbacks.size).toBe(1);
  cancelFlowFrame(d);
  expect(callbacks.size).toBe(0);
  cancelFlowFrame(b);
});
