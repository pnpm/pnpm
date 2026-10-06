import path from 'node:path'

import { packageRootLinkTarget, refToRelative } from '@pnpm/deps.path'
import { DepType, type DepTypes } from '@pnpm/lockfile.detect-dep-types'
import type {
  PackageSnapshot,
  PackageSnapshots,
  TarballResolution,
} from '@pnpm/lockfile.fs'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile.utils'
import { readPackageJsonFromDirSync } from '@pnpm/pkg-manifest.reader'
import type { StoreIndex } from '@pnpm/store.index'
import type { DependencyManifest, DepPath, RegistriesByScope } from '@pnpm/types'
import normalizePath from 'normalize-path'

import { readManifestFromCafs } from './readManifestFromCafs.js'
import { resolvePackagePath } from './resolvePackagePath.js'

export interface GetPkgInfoOpts {
  readonly alias: string
  readonly ref: string
  readonly currentPackages: PackageSnapshots
  readonly peers?: Set<string>
  readonly registriesByScope: RegistriesByScope
  readonly registriesByPrefix?: Record<string, string>
  readonly skipped: Set<string>
  readonly storeDir?: string
  readonly storeIndex?: StoreIndex
  readonly wantedPackages: PackageSnapshots
  readonly virtualStoreDir?: string
  readonly virtualStoreDirMaxLength: number
  readonly depTypes: DepTypes

  /**
   * The base dir if the `ref` argument is a `"link:"` relative path. An
   * absolute `"link:"` path, such as one on another drive on Windows, is used
   * as is.
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

  readonly nodeLinker?: 'hoisted' | 'isolated' | 'pnp'
  readonly hoistedLocations?: Record<string, string[]>
  readonly lockfileDir?: string
}

export function getPkgInfo (opts: GetPkgInfoOpts): { pkgInfo: PackageInfo, readManifest: () => DependencyManifest } {
  const depPath = refToRelative(opts.ref, opts.alias)
  const lockedInfo: LockedPackageInfo = depPath
    ? readLockedPackageInfo(depPath, opts)
    : { name: opts.alias, version: opts.ref, isMissing: false, isSkipped: false }
  const { name } = lockedInfo
  const fullPackagePath = depPath
    ? resolvePackagePath({
      depPath,
      name,
      alias: opts.alias,
      virtualStoreDir: opts.virtualStoreDir ?? '.pnpm',
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      modulesDir: opts.modulesDir,
      parentDir: opts.parentDir,
      nodeLinker: opts.nodeLinker,
      hoistedLocations: opts.hoistedLocations,
      lockfileDir: opts.lockfileDir,
      projectDir: opts.linkedPathBaseDir,
      version: lockedInfo.version,
    })
    : resolveLinkedPath(opts)

  const version = rewriteLinkVersion(lockedInfo.version || opts.ref, fullPackagePath, opts.rewriteLinkVersionDir)
  const pkgInfo = createPackageInfo(opts, { ...lockedInfo, version, path: fullPackagePath })
  return {
    pkgInfo,
    readManifest: () => readManifest(opts, { integrity: lockedInfo.integrity, name, version, path: fullPackagePath }),
  }
}

function rewriteLinkVersion (version: string, fullPackagePath: string, rewriteLinkVersionDir: string | undefined): string {
  if (!version.startsWith('link:') || !rewriteLinkVersionDir) return version
  return `link:${normalizePath(path.relative(rewriteLinkVersionDir, fullPackagePath))}`
}

function readManifest (
  opts: GetPkgInfoOpts,
  pkg: { integrity?: string, name: string, version: string, path: string }
): DependencyManifest {
  if (pkg.integrity && opts.storeDir && opts.storeIndex) {
    const manifest = readManifestFromCafs(opts.storeDir, opts.storeIndex, { integrity: pkg.integrity, name: pkg.name, version: pkg.version })
    if (manifest) return manifest
  }
  return readPackageJsonFromDirSync(pkg.path)
}

interface LockedPackageInfo {
  name: string
  version: string
  isMissing: boolean
  isSkipped: boolean
  resolved?: string
  optional?: true
  integrity?: string
  depType?: DepType
}

function readLockedPackageInfo (depPath: DepPath, opts: GetPkgInfoOpts): LockedPackageInfo {
  const { pkgSnapshot, ...identity } = findPackageSnapshot(depPath, opts)
  return {
    ...identity,
    ...(pkgSnapshot && readSnapshotDetails(depPath, pkgSnapshot, opts)),
    depType: opts.depTypes[depPath],
  }
}

function findPackageSnapshot (
  depPath: DepPath,
  opts: GetPkgInfoOpts
): Pick<LockedPackageInfo, 'name' | 'version' | 'isMissing' | 'isSkipped'> & { pkgSnapshot?: PackageSnapshot } {
  if (opts.currentPackages[depPath]) {
    const pkgSnapshot = opts.currentPackages[depPath]
    const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    return { pkgSnapshot, name, version, isMissing: false, isSkipped: false }
  }
  const pkgSnapshot: PackageSnapshot | undefined = opts.wantedPackages[depPath]
  const { name, version } = pkgSnapshot
    ? nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    : { name: opts.alias, version: opts.ref }
  return { pkgSnapshot, name, version, isMissing: true, isSkipped: opts.skipped.has(depPath) }
}

function readSnapshotDetails (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  opts: GetPkgInfoOpts
): Pick<LockedPackageInfo, 'resolved' | 'optional' | 'integrity'> {
  return {
    resolved: readTarballUrl(depPath, pkgSnapshot, opts),
    optional: pkgSnapshot.optional,
    integrity: 'integrity' in pkgSnapshot.resolution ? pkgSnapshot.resolution.integrity as string : undefined,
  }
}

function readTarballUrl (depPath: string, pkgSnapshot: PackageSnapshot, opts: GetPkgInfoOpts): string | undefined {
  try {
    return (pkgSnapshotToResolution(depPath, pkgSnapshot, { registriesByScope: opts.registriesByScope, registriesByPrefix: opts.registriesByPrefix }) as TarballResolution).tarball
  } catch (err: unknown) {
    // Inspection commands may run without the workspace's registriesByPrefix
    // setting (registries come from .modules.yaml); a named-registry entry
    // whose alias can't be resolved to a URL just has no tarball to show.
    if ((err as { code?: string }).code !== 'ERR_PNPM_MISSING_NAMED_REGISTRY') throw err
    return undefined
  }
}

function createPackageInfo (
  opts: GetPkgInfoOpts,
  info: LockedPackageInfo & { path: string }
): PackageInfo {
  const packageInfo: PackageInfo = {
    alias: opts.alias,
    isMissing: info.isMissing,
    isPeer: Boolean(opts.peers?.has(opts.alias)),
    isSkipped: info.isSkipped,
    name: info.name,
    path: info.path,
    version: info.version,
  }
  if (info.resolved) {
    packageInfo.resolved = info.resolved
  }
  if (info.optional === true) {
    packageInfo.optional = true
  }
  if (info.depType === DepType.DevOnly) {
    packageInfo.dev = true
  } else if (info.depType === DepType.ProdOnly) {
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

function resolveLinkedPath (opts: Pick<GetPkgInfoOpts, 'linkedPathBaseDir' | 'parentDir' | 'ref'>): string {
  const target = packageRootLinkTarget(opts.ref)
  return target != null && opts.parentDir != null
    ? path.join(opts.parentDir, target)
    : path.resolve(opts.linkedPathBaseDir, opts.ref.slice(5))
}
