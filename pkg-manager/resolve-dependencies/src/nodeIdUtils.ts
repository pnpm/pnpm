export function nodeIdContains (nodeId: string, pkgId: string): boolean {
  const pkgIds = splitNodeId(nodeId)
  return pkgIds.includes(pkgId)
}

export function nodeIdContainsSequence (pkgIds: string[], pkgId1: string, pkgId2: string): boolean {
  const pkg1Index = pkgIds.indexOf(pkgId1)
  if (pkg1Index === -1 || pkg1Index === pkgIds.length - 1) return false
  const pkg2Index = pkgIds.lastIndexOf(pkgId2)
  return pkg1Index < pkg2Index && pkg2Index !== pkgIds.length - 1
}

export function createNodeId (parentNodeId: string, pkgId: string): string {
  // using ">" as a separator because it will never be used inside a package ID
  return `${parentNodeId}${pkgId}>`
}

export function splitNodeId (nodeId: string): string[] {
  return nodeId.split('>').slice(1, -1)
}
