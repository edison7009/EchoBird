import type { ReactNode } from 'react';

export function AppPanelNotice({
  children,
  role,
  inline = false,
  error = false,
}: {
  children: ReactNode;
  role?: 'status' | 'alert';
  inline?: boolean;
  error?: boolean;
}) {
  return (
    <div
      role={role}
      className={`flex flex-col items-center justify-center gap-2 break-words px-3 text-center text-xs leading-relaxed ${
        inline ? 'py-1' : 'min-h-28 flex-1 py-3'
      } ${error ? 'text-red-500' : 'text-cyber-text-secondary'}`}
    >
      {children}
    </div>
  );
}
