export function findDemoCharacterIdLeaks(files) {
  const leaks = []
  for (const { path, source } of files) {
    const normalizedPath = path.replaceAll('\\', '/')
    if (!normalizedPath.startsWith('src/ui/')) continue
    for (const match of source.matchAll(/(['"`])(shen|xuan|liu|mask)\1/g)) {
      leaks.push({ path: normalizedPath, id: match[2] })
    }
  }
  return leaks
}

export function findTempDirLeakViolations(files) {
  const violations = []
  const tempDirPattern = /\b(?:tempdir|TempDir)\b/
  const leakPatterns = [
    ['box-leak', /\bBox\s*::\s*leak\s*\(/],
    ['mem-forget', /\b(?:std\s*::\s*)?mem\s*::\s*forget\s*\(/],
    ['path-conversion', /\.\s*into_path\s*\(/],
    ['directory-keep', /\.\s*keep\s*\(/],
  ]

  for (const { path, source } of files) {
    const normalizedPath = path.replaceAll('\\', '/')
    if (!tempDirPattern.test(source)) continue
    for (const [operation, pattern] of leakPatterns) {
      if (pattern.test(source)) violations.push({ path: normalizedPath, operation })
    }
  }
  return violations
}
