export function nodeIdContainsSequence (nodeId: string, pkgId1: string, pkgId2: string) {
  const pkgIds = splitNodeId(nodeId)
  pkgIds.pop()
  const pkg1Index = pkgIds.indexOf(pkgId1)
  if (pkg1Index === -1) return false
  const pkg2Index = pkgIds.lastIndexOf(pkgId2)
  return pkg1Index < pkg2Index
}

export function createNodeId (parentNodeId: string, pkgId: string) {
  // using ">" as a separator because it will never be used inside a package ID
  return `${parentNodeId}${pkgId}>`
}

export function splitNodeId (nodeId: string) {
  return nodeId.slice(1, -1).split('>')
}
