import { TreeNodeId } from '@pnpm/types'

export function serializeTreeNodeId(treeNodeId: TreeNodeId): string {
  switch (treeNodeId.type) {
    case 'importer': {
      // Only serialize known fields from TreeNodeId. TypeScript is duck typed and
      // objects can have any number of unknown extra fields.
      const { type, importerId } = treeNodeId
      return JSON.stringify({ type, importerId })
    }
    case 'package': {
      const { type, depPath } = treeNodeId
      return JSON.stringify({ type, depPath })
    }
  }
}
