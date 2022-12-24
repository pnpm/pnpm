import path from 'path'
import {
  PackageSnapshot,
  PackageSnapshots,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { Registries } from '@pnpm/types'
import { depPathToFilename, refToRelative } from '@pnpm/dependency-path'

export function getPkgInfo (
  opts: {
    alias: string
    modulesDir: string
    ref: string
    currentPackages: PackageSnapshots
    peers?: Set<string>
    registries: Registries
    skipped: Set<string>
    wantedPackages: PackageSnapshots
  }
) {
  let name!: string
  let version!: string
  let resolved: string | undefined
  let dev: boolean | undefined
  let optional: true | undefined
  let isSkipped: boolean = false
  let isMissing: boolean = false
  const depPath = refToRelative(opts.ref, opts.alias)
  if (depPath) {
    let pkgSnapshot!: PackageSnapshot
    if (opts.currentPackages[depPath]) {
      pkgSnapshot = opts.currentPackages[depPath]
      const parsed = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      name = parsed.name
      version = parsed.version
    } else {
      pkgSnapshot = opts.wantedPackages[depPath]
      if (pkgSnapshot) {
        const parsed = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        name = parsed.name
        version = parsed.version
      } else {
        name = opts.alias
        version = opts.ref
      }
      isMissing = true
      isSkipped = opts.skipped.has(depPath)
    }
    resolved = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)['tarball']
    dev = pkgSnapshot.dev
    optional = pkgSnapshot.optional
  } else {
    name = opts.alias
    version = opts.ref
  }
  const packageAbsolutePath = refToRelative(opts.ref, opts.alias)
  const packageInfo = {
    alias: opts.alias,
    isMissing,
    isPeer: Boolean(opts.peers?.has(opts.alias)),
    isSkipped,
    name,
    path: depPath ? path.join(opts.modulesDir, '.pnpm', depPathToFilename(depPath)) : path.join(opts.modulesDir, '..', opts.ref.slice(5)),
    version,
  }
  if (resolved) {
    packageInfo['resolved'] = resolved
  }
  if (optional === true) {
    packageInfo['optional'] = true
  }
  if (typeof dev === 'boolean') {
    packageInfo['dev'] = dev
  }
  return {
    packageAbsolutePath,
    packageInfo,
  }
}
