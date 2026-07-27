export default function Spinner({
  size = "sm",
  className = "",
}: {
  size?: "sm" | "md";
  className?: string;
}) {
  const dims = size === "sm" ? "h-3.5 w-3.5" : "h-6 w-6";
  return (
    <span
      role="status"
      aria-label="Loading"
      className={`inline-block animate-spin rounded-full border-2 border-current border-t-transparent ${dims} ${className}`}
    />
  );
}
