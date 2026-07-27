/** A small colored tag chip. Colors are explicit hex (theme-independent, like
 *  GitHub labels): tinted background + border derived from the tag color. */
export default function TagChip({
  name,
  color,
  onRemove,
}: {
  name: string;
  color: string;
  onRemove?: () => void;
}) {
  return (
    <span
      className="inline-flex max-w-full items-center gap-1 truncate rounded-full border px-1.5 py-0.5 text-[10px] leading-none"
      style={{ color, borderColor: `${color}66`, background: `${color}1f` }}
    >
      {name}
      {onRemove && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
          className="opacity-70 hover:opacity-100"
          title={`Remove tag ${name}`}
        >
          ×
        </button>
      )}
    </span>
  );
}
