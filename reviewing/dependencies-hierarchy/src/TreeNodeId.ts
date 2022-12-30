export type TreeNodeId = TreeNodeIdPackage

/**
 * An npm package depended on externally.
 */
interface TreeNodeIdPackage {
  readonly type: 'package'
  readonly depPath: string
}

export function serializeTreeNodeId (treeNodeId: TreeNodeId): string {
  switch (treeNodeId.type) {
  case 'package': {
    // Only serialize known fields from TreeNodeId. TypeScript is duck typed and
    // objects can have any number of unknown extra fields.
    const { type, depPath } = treeNodeId
    return JSON.stringify({ type, depPath })
  }
  }
}
