// NavItem component
import React from 'react';
import { RoutingHint } from './RoutingToggle';

export interface NavItemProps {
  id?: string;
  icon: React.ReactNode;
  label: string;
  active?: boolean;
  onClick?: () => void;
  color?: 'accent' | 'warning' | 'secondary' | 'blue';
  trailing?: React.ReactNode;
  collapsed?: boolean;
}

export const NavItem = React.memo(
  ({
    id,
    icon,
    label,
    active = false,
    onClick,
    color = 'accent',
    trailing,
    collapsed = false,
  }: NavItemProps) => {
    const colorClasses =
      color === 'warning'
        ? 'bg-cyber-warning/15 text-cyber-warning'
        : color === 'secondary' || color === 'blue'
          ? 'bg-cyber-elevated/75 text-cyber-text'
          : 'bg-cyber-elevated/75 text-cyber-text';
    const button = (
      <button
        id={id}
        type="button"
        aria-label={label}
        aria-current={active ? 'page' : undefined}
        className={`relative flex items-center ${collapsed ? 'mx-auto h-10 w-10 shrink-0 justify-center' : 'w-full gap-3'} p-2 cursor-default text-left transition-colors rounded-lg font-medium ${
          active
            ? colorClasses
            : 'hover:bg-cyber-elevated/50 text-cyber-text-secondary hover:text-cyber-text'
        }`}
        onClick={onClick}
      >
        {icon}
        <span className={collapsed ? 'sr-only' : undefined}>{label}</span>
        {trailing && (
          <span
            className={
              collapsed ? 'absolute right-1 top-1 [&>svg]:h-2 [&>svg]:w-2' : 'ml-auto flex-shrink-0'
            }
          >
            {trailing}
          </span>
        )}
      </button>
    );
    return collapsed ? (
      <RoutingHint hint={label} placement="right" navigation>
        {button}
      </RoutingHint>
    ) : (
      button
    );
  }
);
