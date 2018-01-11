export function nodeIdContainsSequence (nodeId: string, pkgId1: string, pkgId2: string) {
  return nodeId.indexOf(`>${pkgId1}>${pkgId2}>`) !== -1
}

export function createNodeId (parentNodeId: string, pkgId: string) {
  return `${parentNodeId}${pkgId}>`
}

export function splitNodeId (nodeId: string) {
  return nodeId.split('>')
}

export const ROOT_NODE_ID = '>/>'
