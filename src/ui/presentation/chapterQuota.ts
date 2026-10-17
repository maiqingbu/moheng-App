/** Model runs a chapter has used against its limit (owner decision 2026-10-17: manual reset). */
export interface ChapterQuota {
  used: number
  limit: number
  /** Runs a manual reset adds on top of what is already used. */
  allowance: number
}

export function isChapterQuotaExhausted(quota: ChapterQuota | null | undefined): boolean {
  return Boolean(quota && quota.used >= quota.limit)
}

export function chapterQuotaLabel(quota: ChapterQuota): string {
  return `本章已用模型调用 ${quota.used} / ${quota.limit} 次`
}

export function chapterQuotaResetRequest(chapterId: string) {
  return { type: 'production_reset_chapter_quota' as const, payload: { chapterId } }
}
