import "./SkeletonRows.css";

interface SkeletonRowsProps {
  count?: number;
  height?: number;
}

/** A handful of shimmering placeholder rows, shaped like the list/card content that's about to replace them. */
export function SkeletonRows({ count = 4, height = 44 }: SkeletonRowsProps) {
  return (
    <div className="skeleton-rows">
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className="skeleton" style={{ height }} />
      ))}
    </div>
  );
}
