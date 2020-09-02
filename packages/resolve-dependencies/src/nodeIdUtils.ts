export function nodeIdContainsSequence (nodeId: string, pkgId1: string, pkgId2: string) {
  return nodeId.includes(`>${pkgId1}>${pkgId2}>`)
}

export function createNodeId (parentNodeId: string, pkgId: string) {
  return `${parentNodeId}${pkgId}>`
}

export function splitNodeId (nodeId: string) {
  return nodeId.substr(1, nodeId.length - 2).split('>')
}
