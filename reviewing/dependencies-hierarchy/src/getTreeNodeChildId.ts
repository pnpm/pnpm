import { refToRelative } from '@pnpm/dependency-path'
import { TreeNodeId } from './TreeNodeId'

export interface getTreeNodeChildIdOpts {
  readonly dep: {
    readonly alias: string
    readonly ref: string
  }
}

export function getTreeNodeChildId (opts: getTreeNodeChildIdOpts): TreeNodeId | undefined {
  const depPath = refToRelative(opts.dep.ref, opts.dep.alias)
  if (depPath !== null) {
    return { type: 'package', depPath }
  }

  return undefined
}
