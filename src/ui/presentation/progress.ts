/** Formats a completion ratio in [0, 1] as a whole-number percentage. */
export function formatProgressPercent(progress: number): string {
  return `${Math.round(progress * 100)}%`
}
