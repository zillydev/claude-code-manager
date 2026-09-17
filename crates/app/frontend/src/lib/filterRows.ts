/** Row shapes the sidebar's filter menus lay out: which section a row belongs to, which one is picked. */
type Sectioned = { section?: string };
type Pinnable = Sectioned & { selected: boolean; divider?: boolean };

/** The header to draw above `rows[i]`: a section names every row it holds, and heads the first of them. */
export function headerAt<T extends Sectioned>(rows: T[], i: number): string | undefined {
  const { section } = rows[i];
  return section && section !== rows[i - 1]?.section ? section : undefined;
}

/** Whether `rows[i]` is the first row past a section, so a rule can close it — a section that runs
 *  to the end of the list needs none, and one section meeting another is closed by its header's rule. */
export function leavesSection<T extends Sectioned>(rows: T[], i: number): boolean {
  const before = rows[i - 1]?.section;
  return before !== undefined && before !== rows[i].section;
}

/** The selected row moved to the top with a rule under the rest — single-select menus only.
 *  It drops its section on the way: the rows it leaves behind are the ones that still need the header. */
export function pinSelected<T extends Pinnable>(rows: T[]): T[] {
  const i = rows.findIndex((r) => r.selected);
  if (i <= 0) return rows;
  const rest = rows.filter((_, j) => j !== i);
  return [{ ...rows[i], section: undefined }, { ...rest[0], divider: true }, ...rest.slice(1)];
}
