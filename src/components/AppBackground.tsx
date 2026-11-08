import { useBackgroundStore } from '../stores/backgroundStore';
import { DesktopCharacterBackground } from './DesktopCharacterBackground';

export function AppBackground() {
  const mode = useBackgroundStore((state) => state.mode);
  if (mode === 'characters') return <DesktopCharacterBackground />;
  if (mode === 'none') return null;
  const surface =
    mode === 'basalt' ? 'app-background-basalt' : `app-background-material app-background-${mode}`;
  return (
    <div
      aria-hidden="true"
      data-app-background={mode}
      className={`${surface} pointer-events-none absolute inset-0 -z-10`}
    />
  );
}
