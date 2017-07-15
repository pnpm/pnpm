export * from './prune'
export * from './read'
export * from './types'

import prune from './prune'
import write from './write'

export {prune, write}

// for backward compatibility
import {refToRelative} from 'dependency-path'
export const getPkgShortId = refToRelative
