/** Apple-style activity indicator (twelve fading spokes). */
export function Spinner({ size = 28, label }: { size?: number; label?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      role="status"
      aria-label={label}
      className="text-label-secondary"
    >
      {Array.from({ length: 12 }, (_, i) => (
        <rect
          key={i}
          x="11"
          y="2"
          width="2"
          height="6"
          rx="1"
          fill="currentColor"
          transform={`rotate(${i * 30} 12 12)`}
          opacity={0.25}
        >
          <animate
            attributeName="opacity"
            values="1;0.25"
            dur="1s"
            begin={`${(i / 12 - 1).toFixed(3)}s`}
            repeatCount="indefinite"
          />
        </rect>
      ))}
    </svg>
  );
}
