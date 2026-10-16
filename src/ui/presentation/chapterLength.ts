// FX-18: chapter length ranges; mirrors src-tauri/src/chapter_length.rs.
export interface ChapterLengthRange { min: number; max: number }

export const DEFAULT_CHAPTER_LENGTH_RANGE: ChapterLengthRange = { min: 2000, max: 3000 }

export const CHAPTER_LENGTH_PRESETS: ChapterLengthRange[] = [
  { min: 2000, max: 3000 },
  { min: 3000, max: 4000 },
  { min: 4000, max: 5000 },
]

export function isValidChapterLengthRange(range: ChapterLengthRange | null | undefined): range is ChapterLengthRange {
  return !!range && Number.isInteger(range.min) && Number.isInteger(range.max)
    && range.min >= 500 && range.min < range.max && range.max <= 10000
}

/** Adds a per-chapter range to a command payload only when one is chosen and valid. */
export function withChapterLengthRange<T extends Record<string, unknown>>(payload: T, range: ChapterLengthRange | null | undefined): T {
  return isValidChapterLengthRange(range) ? { ...payload, chapterLengthRange: { min: range.min, max: range.max } } : payload
}

export function chapterLengthLabel(range: ChapterLengthRange): string {
  return `${range.min}–${range.max} 字`
}
