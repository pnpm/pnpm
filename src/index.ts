export * from './prune'
export * from './read'
export * from './types'

import prune from './prune'
import write, {
  writeCurrentOnly,
  writeWantedOnly,
} from './write'

export {prune, write, writeWantedOnly, writeCurrentOnly}

// for backward compatibility
import {refToRelative} from 'dependency-path'
export const getPkgShortId = refToRelative
