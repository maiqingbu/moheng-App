export interface CanonGraphPoint {
  x: number
  y: number
}

export function layoutCanonGraphNodes(characterIds: readonly string[]): ReadonlyMap<string, CanonGraphPoint> {
  const sortedIds = [...characterIds].sort((left, right) => left.localeCompare(right))
  const positions = new Map<string, CanonGraphPoint>()

  sortedIds.forEach((id, index) => {
    const angle = -Math.PI / 2 + (2 * Math.PI * index) / sortedIds.length
    positions.set(id, {
      x: 50 + Math.cos(angle) * 36,
      y: 50 + Math.sin(angle) * 36,
    })
  })

  return positions
}
