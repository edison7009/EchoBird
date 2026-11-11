import { expect, it } from 'vitest';
import { featureDemoAt } from './featureTimeline';
it('demonstrates route selection, environment installation, tool installation and PATH configuration before success', () => {
  expect(
    [0, 1300, 2000, 3500, 5000, 7000, 8500, 11000].map((t) => featureDemoAt(t).installStep)
  ).toEqual([0, 1, 2, 3, 4, 5, 6, 0]);
});

it('opens and selects a model in each of the four cards before starting the next round', () => {
  expect(featureDemoAt(700)).toMatchObject({ route: 0, modelOpen: true, modelSelected: false });
  expect(featureDemoAt(2500)).toMatchObject({ route: 0, modelOpen: true, modelSelected: true });
  expect(featureDemoAt(3200).modelOpen).toBe(false);
  expect([0, 4000, 8000, 12000, 16000].map((time) => featureDemoAt(time).route)).toEqual([
    0, 1, 2, 3, 0,
  ]);
  expect(featureDemoAt(16000)).toMatchObject({ modelRound: 1, modelSelected: false });
});
