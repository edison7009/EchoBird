import { useEffect, useState } from 'react';
import type { LocalTool } from '../../api/types';
import { isToolIconReady, preloadDesktopToolIcons, resolvedToolIcon } from '../../utils/toolIcons';

/** Publish one complete visible grid, preserving the previous grid during refresh. */
export function useDesktopTools(tools: LocalTool[], scope: string) {
  const key = JSON.stringify([
    scope,
    tools.map((tool) => [
      tool.id,
      tool.icon,
      tool.name,
      tool.displayName,
      tool.names,
      tool.installed,
    ]),
  ]);
  const ready = tools.every((tool) => isToolIconReady(tool.icon));
  const resolveTools = () => {
    if (!tools.some((tool) => tool.icon && resolvedToolIcon(tool.icon) !== tool.icon)) return tools;
    return tools.map((tool) => {
      const icon = resolvedToolIcon(tool.icon);
      return icon === tool.icon ? tool : { ...tool, icon };
    });
  };
  const [snapshot, setSnapshot] = useState(() => ({
    key: ready ? key : '',
    scope,
    tools: ready ? resolveTools() : [],
  }));
  if (ready && snapshot.key !== key) {
    setSnapshot({ key, scope, tools: resolveTools() });
  }

  useEffect(() => {
    if (ready) return;
    let active = true;
    // Capture this request's tools; obsolete sources never replace a newer grid.
    void preloadDesktopToolIcons(tools).then((prepared) => {
      if (active) setSnapshot({ key, scope, tools: prepared });
    });
    return () => {
      active = false;
    };
    // The key contains every field affecting tile appearance and ordering.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, ready]);

  const pending = !ready && snapshot.key !== key;
  return {
    key,
    tools: ready ? resolveTools() : snapshot.scope === scope ? snapshot.tools : [],
    pending,
  };
}
