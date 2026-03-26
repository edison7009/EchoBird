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
        ? 'bg-cyber-warning text-black font-bold'
        : color === 'secondary' || color === 'blue'
            ? 'bg-cyber-accent-secondary text-black font-bold'
            : 'bg-cyber-accent text-black font-bold';
    return (
        <div
            className={`flex items-center gap-3 p-2 cursor-pointer transition-all rounded-lg ${active
                ? colorClasses
                : 'hover:bg-cyber-input text-cyber-text-secondary'
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
