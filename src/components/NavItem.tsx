// NavItem component
import React from 'react';

export interface NavItemProps {
    icon: React.ReactNode;
    label: string;
    active?: boolean;
    onClick?: () => void;
    color?: 'accent' | 'warning' | 'secondary' | 'blue';
    badge?: boolean;
}

export const NavItem = React.memo(({ icon, label, active = false, onClick, color = 'accent', badge = false }: NavItemProps) => {
    const colorClasses = color === 'warning'
        ? 'bg-cyber-warning/15 text-cyber-warning font-medium'
        : color === 'secondary' || color === 'blue'
            ? 'bg-cyber-elevated text-cyber-text font-medium'
            : 'bg-cyber-elevated text-cyber-text font-medium';
    return (
        <div
            className={`flex items-center gap-3 p-2 cursor-pointer transition-colors rounded-lg ${active
                ? colorClasses
                : 'hover:bg-cyber-elevated/50 text-cyber-text-secondary hover:text-cyber-text'
                }`}
            onClick={onClick}
        >
            {icon}
            <span>{label}</span>
            {badge && !active && (
                <span className="w-2 h-2 rounded-full bg-red-500 animate-pulse flex-shrink-0" />
            )}
        </div>
    );
});
