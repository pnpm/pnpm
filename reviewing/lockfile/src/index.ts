import type { Graph } from './graph/types'
import * as v5 from './pnpm/v5'
import * as v6 from './pnpm/v6'
import { createBuilder } from './graph/builder'

export function createLockfileGraph (lockfile: any): Graph { // eslint-disable-line
  const version = lockfile.lockfileVersion
  if (!version) {
    throw Error('lockfileVersion is not defined')
  }
  if (v6.isExperimentalInlineSpecifiersFormat(lockfile)) {
    return v6.graphBuilder(lockfile)
  } else if (typeof version === 'number' && version >= 5 && version < 6) {
    return v5.graphBuilder(lockfile)
  } else {
    throw Error('unsupported lockfile version')
  }
}

export const createPnpmV5LockfileGraph = v5.graphBuilder
export const createPnpmV6LockfileGraph = v6.graphBuilder
export const createGraphBuilder = createBuilder
export type { Graph }
export { diffGraph } from './graph/diff'
