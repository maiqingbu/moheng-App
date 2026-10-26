// FX-18: chapter length ranges; mirrors src-tauri/src/chapter_length.rs.
export interface ChapterLengthRange { min: number; max: number }

export const DEFAULT_CHAPTER_LENGTH_RANGE: ChapterLengthRange = { min: 2000, max: 3000 }

export const CHAPTER_LENGTH_PRESETS: ChapterLengthRange[] = [
  { min: 2000, max: 3000 },
  { min: 3000, max: 4000 },
  { min: 4000, max: 5000 },
]

/** LD-1.5: the platforms a work can target and the characters a chapter aims for there (PLATFORM_CHAPTER_TARGETS). */
export const PLATFORM_CHAPTER_TARGETS: { name: string, target: number }[] = [
  { name: '番茄小说', target: 2000 },
  { name: '七猫小说', target: 2000 },
  { name: '起点中文网', target: 3000 },
  { name: '纵横中文网', target: 3000 },
  { name: '晋江文学城', target: 3000 },
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
