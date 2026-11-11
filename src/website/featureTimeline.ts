// Presentation state only: no commands, downloads, account reads or writes.
export function featureDemoAt(time: number) {
  const elapsed = Math.max(0, time);
  const installTime = elapsed % 11000;
  const modelTime = elapsed % 4000;
  return {
    route: Math.floor(elapsed / 4000) % 4,
    modelRound: Math.floor(elapsed / 16000),
    modelOpen: modelTime >= 650 && modelTime < 3100,
    modelSelected: modelTime >= 2400,
    installStep:
      installTime < 1200
        ? 0
        : installTime < 1800
          ? 1
          : installTime < 3200
            ? 2
            : installTime < 4800
              ? 3
              : installTime < 6400
                ? 4
                : installTime < 8000
                  ? 5
                  : 6,
  };
}
