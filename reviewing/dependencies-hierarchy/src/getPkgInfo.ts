import path from 'path'
import {
  type PackageSnapshot,
  type PackageSnapshots,
  type TarballResolution,
} from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile.utils'
import { type DepTypes, DepType } from '@pnpm/lockfile.detect-dep-types'
import { type DependencyManifest, type Registries } from '@pnpm/types'
import { refToRelative } from '@pnpm/dependency-path'
import { readPackageJsonFromDirSync } from '@pnpm/read-package-json'
import normalizePath from 'normalize-path'
import { readManifestFromCafs } from './readManifestFromCafs.js'
import { resolvePackagePath } from './resolvePackagePath.js'

export interface GetPkgInfoOpts {
  readonly alias: string
  readonly ref: string
  readonly currentPackages: PackageSnapshots
  readonly peers?: Set<string>
  readonly registries: Registries
  readonly skipped: Set<string>
  readonly storeDir?: string
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

  /**
   * The node_modules directory to resolve symlinks from when using global virtual store.
   * This is used for top-level dependencies.
   */
  readonly modulesDir?: string

  /**
   * The resolved path of the parent package. When provided, the symlink resolution
   * will use the parent's node_modules directory instead of the top-level modulesDir.
   * This is needed for subdependencies when using global virtual store.
   */
  readonly parentDir?: string
}

export function getPkgInfo (opts: GetPkgInfoOpts): { pkgInfo: PackageInfo, readManifest: () => DependencyManifest } {
  let name!: string
  let version: string
  let resolved: string | undefined
  let depType: DepType | undefined
  let optional: true | undefined
  let isSkipped: boolean = false
  let isMissing: boolean = false
  let integrity: string | undefined
  const depPath = refToRelative(opts.ref, opts.alias)
  if (depPath) {
    let pkgSnapshot: PackageSnapshot | undefined
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
    if (pkgSnapshot) {
      resolved = (pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries) as TarballResolution).tarball
      optional = pkgSnapshot.optional
      if ('integrity' in pkgSnapshot.resolution) {
        integrity = pkgSnapshot.resolution.integrity as string
      }
    }
    depType = opts.depTypes[depPath]
  } else {
    name = opts.alias
    version = opts.ref
  }
  if (!version) {
    version = opts.ref
  }
  const fullPackagePath = depPath
    ? resolvePackagePath({
      depPath,
      name,
      alias: opts.alias,
      virtualStoreDir: opts.virtualStoreDir ?? '.pnpm',
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      modulesDir: opts.modulesDir,
      parentDir: opts.parentDir,
    })
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
  return {
    pkgInfo: packageInfo,
    readManifest: () => {
      if (integrity && opts.storeDir) {
        const manifest = readManifestFromCafs(opts.storeDir, { integrity, name, version })
        if (manifest) return manifest
      }
      return readPackageJsonFromDirSync(fullPackagePath)
    },
  }
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
