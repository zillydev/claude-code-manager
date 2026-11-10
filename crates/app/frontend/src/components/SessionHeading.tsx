/** Left side of a session header row: project, title, optional raw-title subtitle. The title gives way first. */
export function SessionHeading({
  project,
  title,
  subtitle,
}: {
  project?: string;
  title?: string;
  subtitle?: string;
}) {
  return (
    <>
      {project && (
        <span
          className="shrink-0 max-w-[16rem] truncate font-medium text-[var(--color-accent)]"
          title={project}
        >
          {project}
        </span>
      )}
      {title && (
        <span className="min-w-0 truncate font-medium text-[var(--color-text)]" title={title}>
          {title}
        </span>
      )}
      {subtitle && (
        <span className="min-w-0 truncate text-xs text-[var(--color-text-dim)]" title={subtitle}>
          {subtitle}
        </span>
      )}
    </>
  );
}
