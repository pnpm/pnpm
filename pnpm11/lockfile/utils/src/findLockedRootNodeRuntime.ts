import type { LockfileObject } from '@pnpm/lockfile.types'
import { DEPENDENCIES_FIELDS, type ProjectId } from '@pnpm/types'

export interface LockedNodeRuntime {
  specifier?: string
  version: string
}

const RUNTIME_PREFIX = 'runtime:'

/**
 * The Node.js runtime the root project's `node` dependency is locked to: the
 * Node.js pnpm installs for the project.
 */
export function findLockedRootNodeRuntime (lockfile: LockfileObject): LockedNodeRuntime | undefined {
  const rootImporter = lockfile.importers['.' as ProjectId]
  if (rootImporter == null) return undefined
  for (const depType of DEPENDENCIES_FIELDS) {
    const ref = rootImporter[depType]?.node
    if (ref?.startsWith(RUNTIME_PREFIX)) {
      return {
        specifier: rootImporter.specifiers?.node,
        version: ref.slice(RUNTIME_PREFIX.length),
      }
    }
  }
  return undefined
}
