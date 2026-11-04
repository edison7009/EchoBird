// All flowing controls share one native frame request, while each owner can
// cancel its pending paint independently when hidden or unmounted.
const pending = new Map<number, FrameRequestCallback>();
let frame = 0;
let nextId = 0;

export function requestFlowFrame(callback: FrameRequestCallback): number {
  const id = ++nextId;
  pending.set(id, callback);
  if (!frame) {
    frame = requestAnimationFrame((now) => {
      frame = 0;
      const callbacks = [...pending.values()];
      pending.clear();
      callbacks.forEach((paint) => paint(now));
    });
  }
  return id;
}

export function cancelFlowFrame(id: number) {
  pending.delete(id);
  if (!pending.size && frame) {
    cancelAnimationFrame(frame);
    frame = 0;
  }
}
