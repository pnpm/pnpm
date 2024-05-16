let nodeIdCounter = 0

type Brand<K, T> = K & { __brand: T }

export type NodeId = Brand<string, 'nodeId'>

export function nextNodeId (): NodeId {
  return (++nodeIdCounter).toString() as NodeId
}
