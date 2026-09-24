import path from 'node:path'

import { refToRelative } from '@pnpm/deps.path'
import { getLockfileImporterId, type ProjectSnapshot } from '@pnpm/lockfile.fs'

import type { TreeNodeId } from './TreeNodeId.js'

export interface GetTreeNodeChildIdOpts {
  readonly parentId: TreeNodeId
  readonly dep: {
    readonly alias: string
    readonly ref: string
  }
  readonly lockfileDir: string
  readonly importers: Record<string, ProjectSnapshot>
}

export function getTreeNodeChildId (opts: GetTreeNodeChildIdOpts): TreeNodeId | undefined {
  const depPath = refToRelative(opts.dep.ref, opts.dep.alias)
  if (depPath !== null) {
    return { type: 'package', depPath }
  }

  switch (opts.parentId.type) {
    case 'importer': {
    // This should be a link given depPath is null.
    //
    // TODO: Consider updating refToRelative (or writing a new function) to
    // return an enum so there's no implicit assumptions.
      const linkValue = opts.dep.ref.slice('link:'.length)

      // It's a bit roundabout to prepend the lockfile dir only to remove it
      // through getLockfileImporterId, but we can be more certain the right
      // importerId is created by reusing the getLockfileImporterId function.
      const absoluteLinkedPath = path.join(opts.lockfileDir, opts.parentId.importerId, linkValue)
      const childImporterId = getLockfileImporterId(opts.lockfileDir, absoluteLinkedPath)

      if (opts.importers[childImporterId] != null) {
        return { type: 'importer', importerId: childImporterId }
      }
      // A 'link:' reference may refer to a package outside of the pnpm workspace.
      // Return undefined in that case since it would be difficult to list/traverse
      // that package outside of the pnpm workspace.
      const publishingImporterId = findImporterByPublishDirectory(opts.lockfileDir, opts.importers, childImporterId)
      return publishingImporterId == null
        ? undefined
        : { type: 'importer', importerId: publishingImporterId }
    }
    case 'package':
    // In theory an external package could be overridden to link to a
    // dependency in the pnpm workspace. Avoid traversing through this
    // edge case for now.
      return undefined
  }
}

/**
 * The importer that dependents link through its publish directory: a project
 * with `publishConfig.directory` is linked there unless
 * `publishConfig.linkDirectory` is false.
 */
function findImporterByPublishDirectory (
  lockfileDir: string,
  importers: Record<string, ProjectSnapshot>,
  linkedImporterId: string
): string | undefined {
  return Object.keys(importers).find((importerId) => {
    const { publishDirectory, linkDirectory } = importers[importerId]
    return publishDirectory != null &&
      linkDirectory !== false &&
      getLockfileImporterId(lockfileDir, path.join(lockfileDir, importerId, publishDirectory)) === linkedImporterId
  })
}
