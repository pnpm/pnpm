export function nodeIdContainsSequence (nodeId: string, pkgId1: string, pkgId2: string) {
  const pkgIds = splitNodeId(nodeId)
  pkgIds.pop()
  const pkg1Index = pkgIds.indexOf(pkgId1)
  if (pkg1Index === -1) return false
  const pkg2Index = pkgIds.indexOf(pkgId2)
  return pkg2Index > -1 && pkg1Index < pkg2Index
}

export function createNodeId (parentNodeId: string, pkgId: string) {
  // using ">" as a separator because it will never be used inside a package ID
  return `${parentNodeId}${pkgId}>`
}

export function splitNodeId (nodeId: string) {
  return nodeId.substr(1, nodeId.length - 2).split('>')
}
