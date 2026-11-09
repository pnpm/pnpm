import { LOCKFILE_VERSION } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type {
  EnvImporterSnapshot,
  EnvLockfile,
  LockfilePackageInfo,
  LockfilePackageSnapshot,
  LockfileResolution,
  SpecifierAndResolution,
} from '@pnpm/lockfile.types'

/**
 * The major of the lockfile format pnpm 13 writes by default. pnpm 11 reads
 * it only in the env document, so that it can switch to the pnpm version a
 * project pins even when the project's lockfile is in that format.
 */
const NEXT_FORMAT_MAJOR = '13'

/** A dependency of an importer: the specifier alone when the version follows from it. */
type NextDependency = string | { specifier: string, version?: string, with?: Record<string, string> }

/** A reference from a package: a version, or a version with the bindings that name the snapshot. */
type NextReference = string | { version: string, with?: Record<string, string> }

/** A declared or bound peer. */
type NextPeer = string | { specifier?: string, version?: string, optional?: true, below?: true }

interface NextPackage {
  integrity?: string
  resolution?: LockfileResolution
  version?: string
  engines?: LockfilePackageInfo['engines']
  cpu?: string[]
  os?: string[]
  libc?: string[]
  hasBin?: true
  bundledDependencies?: string[] | boolean
  peerDependencies?: Record<string, NextPeer>
  dependencies?: Record<string, NextReference>
  optionalDependencies?: Record<string, NextReference>
  optional?: true
  snapshots?: unknown[]
}

interface NextEnvImporter {
  configDependencies?: Record<string, NextDependency>
  packageManagerDependencies?: Record<string, NextDependency>
}

export interface NextEnvDocument {
  lockfileVersion: string
  /** `projects`: the next format's name for today's `importers`. */
  projects?: Record<string, NextEnvImporter>
  packages?: Record<string, NextPackage>
}

export function isNextFormatDocument (document: Record<string, unknown>): document is Record<string, unknown> & NextEnvDocument {
  return typeof document.lockfileVersion === 'string' &&
    document.lockfileVersion.split('.')[0] === NEXT_FORMAT_MAJOR
}

/**
 * The env document of a lockfile in the next format as an env lockfile of
 * today's shape: each `packages` entry becomes a `packages` and a
 * `snapshots` entry, a dependency written as its specifier alone gets the
 * version the specifier implies, and `peerDependencies` splits into today's
 * ranges, `peerDependenciesMeta`, and the bound peers' edges. The result
 * declares today's version, since it has today's shape.
 */
export function envLockfileFromNextFormat (document: NextEnvDocument): EnvLockfile {
  const importers: Record<string, EnvImporterSnapshot> = {}
  for (const [id, importer] of Object.entries(document.projects ?? {})) {
    importers[id] = { configDependencies: resolvedGroup(importer.configDependencies ?? {}) }
    if (importer.packageManagerDependencies) {
      importers[id].packageManagerDependencies = resolvedGroup(importer.packageManagerDependencies)
    }
  }
  importers['.'] ??= { configDependencies: {} }
  const packages: EnvLockfile['packages'] = {}
  const snapshots: EnvLockfile['snapshots'] = {}
  for (const [key, entry] of Object.entries(document.packages ?? {})) {
    if (entry.snapshots != null) {
      throw new PnpmError('NEXT_LOCKFILE_ENV_SNAPSHOTS', `The env document of ${LOCKFILE_VERSION_NAME} lists several snapshots for ${key}, which a config dependency cannot have`)
    }
    packages[key] = packageInfo(key, entry)
    snapshots[key] = packageSnapshot(entry)
  }
  return {
    lockfileVersion: LOCKFILE_VERSION,
    importers: importers as EnvLockfile['importers'],
    packages,
    snapshots,
  }
}

const LOCKFILE_VERSION_NAME = 'pnpm-lock.yaml'

function resolvedGroup (group: Record<string, NextDependency>): Record<string, SpecifierAndResolution> {
  const resolved: Record<string, SpecifierAndResolution> = {}
  for (const [name, dependency] of Object.entries(group)) {
    resolved[name] = typeof dependency === 'string'
      ? { specifier: dependency, version: impliedVersion(name, dependency) }
      : { specifier: dependency.specifier, version: dependency.version ?? impliedVersion(name, dependency.specifier) }
  }
  return resolved
}

/**
 * The version a specifier written alone implies: itself for an exact version,
 * and `name@1.2.3` for `npm:name@1.2.3`. The env document pins exact
 * versions, so a specifier that implies no version is an error.
 */
function impliedVersion (name: string, specifier: string): string {
  if (!specifier.startsWith('npm:')) return specifier
  const target = specifier.slice('npm:'.length)
  if (target.indexOf('@', 1) === -1) {
    throw new PnpmError('NEXT_LOCKFILE_UNDERIVABLE_VERSION', `The env document of ${LOCKFILE_VERSION_NAME} does not say which version ${name} resolves to: nothing follows from the specifier ${specifier}`)
  }
  return target
}

function packageInfo (key: string, entry: NextPackage): LockfilePackageInfo {
  const info: LockfilePackageInfo = { resolution: resolutionOf(key, entry) }
  if (entry.version !== undefined) info.version = entry.version
  if (entry.engines !== undefined) info.engines = entry.engines
  if (entry.cpu !== undefined) info.cpu = entry.cpu
  if (entry.os !== undefined) info.os = entry.os
  if (entry.libc !== undefined) info.libc = entry.libc
  if (entry.hasBin !== undefined) info.hasBin = entry.hasBin
  if (entry.bundledDependencies !== undefined) info.bundledDependencies = entry.bundledDependencies
  const peers = declaredPeers(entry.peerDependencies ?? {})
  if (peers.ranges) info.peerDependencies = peers.ranges
  if (peers.meta) info.peerDependenciesMeta = peers.meta
  return info
}

/** The resolution, with the entry's `integrity` folded back into it. */
function resolutionOf (key: string, { integrity, resolution }: NextPackage): LockfileResolution {
  if (resolution == null) {
    if (integrity == null) {
      throw new PnpmError('NEXT_LOCKFILE_ENV_RESOLUTION', `The env document of ${LOCKFILE_VERSION_NAME} records no resolution for ${key}`)
    }
    return { integrity }
  }
  return integrity != null && 'tarball' in resolution ? { ...resolution, integrity } : resolution
}

function declaredPeers (peers: Record<string, NextPeer>): {
  ranges?: Record<string, string>
  meta?: Record<string, { optional: true }>
} {
  const ranges: Record<string, string> = {}
  const meta: Record<string, { optional: true }> = {}
  for (const [name, peer] of Object.entries(peers)) {
    if (typeof peer === 'string') {
      ranges[name] = peer
      continue
    }
    if (peer.specifier != null) ranges[name] = peer.specifier
    if (peer.optional) meta[name] = { optional: true }
  }
  return {
    ranges: Object.keys(ranges).length > 0 ? ranges : undefined,
    meta: Object.keys(meta).length > 0 ? meta : undefined,
  }
}

/**
 * Today's snapshot: the references' versions, a bound own peer among the
 * dependencies it was linked as, and a peer bound below the package among
 * `transitivePeerDependencies`.
 */
function packageSnapshot (entry: NextPackage): LockfilePackageSnapshot {
  const dependencies = versionsOf(entry.dependencies)
  const optionalDependencies = versionsOf(entry.optionalDependencies)
  const transitivePeerDependencies = addBoundPeers(entry.peerDependencies ?? {}, dependencies, optionalDependencies)
  const snapshot: LockfilePackageSnapshot = {}
  if (Object.keys(dependencies).length > 0) snapshot.dependencies = dependencies
  if (Object.keys(optionalDependencies).length > 0) snapshot.optionalDependencies = optionalDependencies
  if (transitivePeerDependencies.length > 0) snapshot.transitivePeerDependencies = transitivePeerDependencies.sort()
  if (entry.optional) snapshot.optional = true
  return snapshot
}

/**
 * Adds each bound own peer to the dependencies or the optional dependencies
 * and returns the names of the peers bound below the package.
 */
function addBoundPeers (
  peers: Record<string, NextPeer>,
  dependencies: Record<string, string>,
  optionalDependencies: Record<string, string>
): string[] {
  const below: string[] = []
  for (const [name, peer] of Object.entries(peers)) {
    if (typeof peer === 'string' || peer.version == null) continue
    if (peer.below) {
      below.push(name)
    } else if (peer.optional) {
      optionalDependencies[name] = peer.version
    } else {
      dependencies[name] = peer.version
    }
  }
  return below
}

function versionsOf (references: Record<string, NextReference> | undefined): Record<string, string> {
  const versions: Record<string, string> = {}
  for (const [name, reference] of Object.entries(references ?? {})) {
    versions[name] = typeof reference === 'string' ? reference : reference.version
  }
  return versions
}
