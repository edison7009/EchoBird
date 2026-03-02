const EchobirdIcon = ({ size = 64, className = '' }) => (
    <svg
        xmlns="http://www.w3.org/2000/svg"
        viewBox="0 0 128 128"
        width={size}
        height={size}
        className={className}
    >
        {/* Black background */}
        <rect width="128" height="128" fill="#000000" />

        {/* Head - Pointing up */}
        <rect x="56" y="24" width="16" height="8" fill="#00FF9D" />
        <rect x="52" y="32" width="24" height="8" fill="#00FF9D" />
        <rect x="48" y="40" width="32" height="16" fill="#00FF9D" />

        {/* Eyes - Black */}
        <rect x="56" y="44" width="6" height="6" fill="#000000" />
        <rect x="66" y="44" width="6" height="6" fill="#000000" />

        {/* Left claws (Spreading top-left, left, bottom-left) */}
        {/* Top-left claw */}
        <rect x="40" y="48" width="8" height="8" fill="#00FF9D" />
        <rect x="32" y="44" width="8" height="8" fill="#00FF9D" />
        <rect x="24" y="40" width="8" height="8" fill="#00FF9D" />

        {/* Mid-left claw */}
        <rect x="36" y="56" width="12" height="8" fill="#00FF9D" />
        <rect x="24" y="60" width="12" height="8" fill="#00FF9D" />
        <rect x="16" y="64" width="8" height="8" fill="#00FF9D" />

        {/* Bottom-left claw */}
        <rect x="40" y="64" width="8" height="8" fill="#00FF9D" />
        <rect x="32" y="72" width="8" height="8" fill="#00FF9D" />
        <rect x="24" y="80" width="8" height="8" fill="#00FF9D" />

        {/* Right claws (Spreading top-right, right, bottom-right) */}
        {/* Top-right claw */}
        <rect x="80" y="48" width="8" height="8" fill="#00FF9D" />
        <rect x="88" y="44" width="8" height="8" fill="#00FF9D" />
        <rect x="96" y="40" width="8" height="8" fill="#00FF9D" />

        {/* Mid-right claw */}
        <rect x="80" y="56" width="12" height="8" fill="#00FF9D" />
        <rect x="92" y="60" width="12" height="8" fill="#00FF9D" />
        <rect x="104" y="64" width="8" height="8" fill="#00FF9D" />

        {/* Bottom-right claw */}
        <rect x="80" y="64" width="8" height="8" fill="#00FF9D" />
        <rect x="88" y="72" width="8" height="8" fill="#00FF9D" />
        <rect x="96" y="80" width="8" height="8" fill="#00FF9D" />

        {/* Bottom claws (Spreading down, three) */}
        {/* Bottom-left claw */}
        <rect x="52" y="56" width="8" height="8" fill="#00FF9D" />
        <rect x="48" y="64" width="8" height="8" fill="#00FF9D" />
        <rect x="44" y="72" width="8" height="12" fill="#00FF9D" />
        <rect x="40" y="84" width="8" height="12" fill="#00FF9D" />

        {/* Bottom-middle claw */}
        <rect x="60" y="56" width="8" height="8" fill="#00FF9D" />
        <rect x="60" y="64" width="8" height="8" fill="#00FF9D" />
        <rect x="60" y="72" width="8" height="12" fill="#00FF9D" />
        <rect x="60" y="84" width="8" height="16" fill="#00FF9D" />

        {/* Bottom-right claw */}
        <rect x="68" y="56" width="8" height="8" fill="#00FF9D" />
        <rect x="72" y="64" width="8" height="8" fill="#00FF9D" />
        <rect x="76" y="72" width="8" height="12" fill="#00FF9D" />
        <rect x="80" y="84" width="8" height="12" fill="#00FF9D" />
    </svg>
);

export default EchobirdIcon;
