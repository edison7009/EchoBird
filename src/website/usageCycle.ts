const styles = ['row', 'column', 'dial', 'list', 'line'] as const;
export function usageCycleAt(time: number) {
  const position = Math.max(0, time);
  const elapsed = position % 3600;
  return {
    style: styles[Math.floor(position / 3600) % styles.length],
    expanded: elapsed >= 500,
    fading: elapsed >= 3250,
  };
}
