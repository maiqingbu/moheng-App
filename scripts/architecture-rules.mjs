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
