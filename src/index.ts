export * from './prune'
export * from './read'
export * from './types'

import existsWanted from './existsWanted'
import nameVerFromPkgSnapshot from './nameVerFromPkgSnapshot'
import pkgSnapshotToResolution from './pkgSnapshotToResolution'
import satisfiesPackageJson from './satisfiesPackageJson'
import write, {
  writeCurrentOnly,
  writeWantedOnly,
} from './write'

export {
  existsWanted,
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
  satisfiesPackageJson,
  write,
  writeWantedOnly,
  writeCurrentOnly,
}

// for backward compatibility
import {refToRelative} from 'dependency-path'
export const getPkgShortId = refToRelative
