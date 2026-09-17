export type PreciseStatusChoice = "unset" | "auto" | "on" | "off";

/** The sidebar announces an automatic install once: while the app enabled it on its own and it wasn't dismissed. */
export function showsPreciseStatusNotice(choice: PreciseStatusChoice | null, dismissed: boolean): boolean {
  return choice === "auto" && !dismissed;
}
