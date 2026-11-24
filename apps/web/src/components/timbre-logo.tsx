export function TimbreLogo({
  className,
  compact,
}: {
  className?: string;
  compact?: boolean;
}) {
  return (
    <img
      src="/logo.svg"
      alt="Timbre"
      width={1424}
      height={203}
      className={className}
      data-compact={compact ? "true" : undefined}
    />
  );
}
