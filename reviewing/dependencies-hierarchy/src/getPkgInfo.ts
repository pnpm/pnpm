import path from 'path'
import {
  type PackageSnapshot,
  type PackageSnapshots,
  type TarballResolution,
} from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { type DepTypes, DepType } from '@pnpm/lockfile.detect-dep-types'
import { type Registries } from '@pnpm/types'
import { depPathToFilename, refToRelative } from '@pnpm/dependency-path'
import normalizePath from 'normalize-path'

export interface GetPkgInfoOpts {
  readonly alias: string
  readonly ref: string
  readonly currentPackages: PackageSnapshots
  readonly peers?: Set<string>
  readonly registries: Registries
  readonly skipped: Set<string>
  readonly wantedPackages: PackageSnapshots
  readonly virtualStoreDir?: string
  readonly virtualStoreDirMaxLength: number
  readonly depTypes: DepTypes

  /**
   * The base dir if the `ref` argument is a `"link:"` relative path.
   */
  readonly linkedPathBaseDir: string

  /**
   * If the `ref` argument is a `"link:"` relative path, the ref is reused for
   * the version field. (Since the true semver may not be known.)
   *
   * Optionally rewrite this relative path to a base dir before writing it to
   * version.
   */
  readonly rewriteLinkVersionDir?: string
}

export function getPkgInfo (opts: GetPkgInfoOpts): PackageInfo {
  let name!: string
  let version: string
  let resolved: string | undefined
  let depType: DepType | undefined
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
    resolved = (pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries) as TarballResolution).tarball
    depType = opts.depTypes[depPath]
    optional = pkgSnapshot.optional
  } else {
    name = opts.alias
    version = opts.ref
  }
  if (!version) {
    version = opts.ref
  }
  const fullPackagePath = depPath
    ? path.join(opts.virtualStoreDir ?? '.pnpm', depPathToFilename(depPath, opts.virtualStoreDirMaxLength), 'node_modules', name)
    : path.join(opts.linkedPathBaseDir, opts.ref.slice(5))

  if (version.startsWith('link:') && opts.rewriteLinkVersionDir) {
    version = `link:${normalizePath(path.relative(opts.rewriteLinkVersionDir, fullPackagePath))}`
  }

  const packageInfo: PackageInfo = {
    alias: opts.alias,
    isMissing,
    isPeer: Boolean(opts.peers?.has(opts.alias)),
    isSkipped,
    name,
    path: fullPackagePath,
    version,
  }
  if (resolved) {
    packageInfo.resolved = resolved
  }
  if (optional === true) {
    packageInfo.optional = true
  }
  if (depType === DepType.DevOnly) {
    packageInfo.dev = true
  } else if (depType === DepType.ProdOnly) {
    packageInfo.dev = false
  }
  return packageInfo
}

interface PackageInfo {
  alias: string
  isMissing: boolean
  isPeer: boolean
  isSkipped: boolean
  name: string
  path: string
  version: string
  resolved?: string
  optional?: true
  dev?: boolean
}
