let nodeIdCounter = 0

type Brand<Base, Tag> = Base & { __brand: Tag }

export type NodeId = Brand<string | number, 'nodeId'>

export function nextNodeId (): NodeId {
  return ++nodeIdCounter as NodeId
}
