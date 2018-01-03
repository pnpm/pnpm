export * from './prune'
export * from './read'
export * from './types'

import prune from './prune'
import write, {writeWantedOnly} from './write'

export {prune, write, writeWantedOnly}

// for backward compatibility
import {refToRelative} from 'dependency-path'
export const getPkgShortId = refToRelative
