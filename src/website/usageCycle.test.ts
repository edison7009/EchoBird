import { expect, it } from 'vitest';
import { usageCycleAt } from './usageCycle';
it('opens one native style, holds it, then fades before the next style', () => {
  expect(usageCycleAt(0)).toEqual({ style: 'row', expanded: false, fading: false });
  expect(usageCycleAt(600)).toEqual({ style: 'row', expanded: true, fading: false });
  expect(usageCycleAt(3300)).toEqual({ style: 'row', expanded: true, fading: true });
  expect(usageCycleAt(3600)).toEqual({ style: 'column', expanded: false, fading: false });
});
it('shows all five styles and returns to the first without changing stage height', () => {
  expect([0, 3600, 7200, 10800, 14400].map((time) => usageCycleAt(time).style)).toEqual([
    'row',
    'column',
    'dial',
    'list',
    'line',
  ]);
  expect(usageCycleAt(18000)).toEqual(usageCycleAt(0));
});
