import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot, PackageSnapshots, ResolvedDependencies } from '@pnpm/lockfile.fs'
import { isGitHostedTarballUrl, nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { Resolution, ResolutionPolicyViolation } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'
import semver from 'semver'

import {
  createParentPeerState,
  isPeerSatisfactionEdge,
  type ParentPeerState,
  type PeerCheckContext,
} from './verifyLockfileExoticSubdepsPeers.js'
import {
  buildExoticSubdepsScopeFromContext,
  buildExoticSubdepsScopeFromProjectDirs,
  canTrustPolicyIdentities,
  collectAllowedPeerProviders,
  collectWorkspaceDirectories,
  computeExoticSubdepsScopeDigest,
  exoticSubdepsCacheIdentity,
  type ExoticSubdepsScope,
  isWorkspaceSnapshot,
  lookupPackageSnapshot,
  type NormalizedExoticSubdepsScope,
  normalizeExoticSubdepsScope,
  snapshotPolicyIdentities,
} from './verifyLockfileExoticSubdepsScope.js'
import type { VerifyLockfileResolutionsOptions } from './verifyLockfileResolutions.js'

export {
  buildExoticSubdepsScopeFromContext,
  buildExoticSubdepsScopeFromProjectDirs,
  canTrustPolicyIdentities,
  computeExoticSubdepsScopeDigest,
  exoticSubdepsCacheIdentity,
  type ExoticSubdepsScope,
  snapshotPolicyIdentities,
}

export const EXOTIC_SUBDEP_VIOLATION_CODE = 'EXOTIC_SUBDEP'

const DEFAULT_NODE_PREFIXES = [
  'https://nodejs.org/download/',
  'https://nodejs.org/dist/',
  'https://unofficial-builds.nodejs.org/download/release/',
]

const DEFAULT_DENO_PREFIXES = ['https://github.com/denoland/deno/releases/download/']

const DEFAULT_BUN_PREFIXES = ['https://github.com/oven-sh/bun/releases/download/']

const ENCODED_PATH_ESCAPE_RE = /%(?:2e|2f|5c)/i

export interface ExoticSubdepScanResult {
  violations: ResolutionPolicyViolation[]
  skippedReplaced: boolean
}

interface ScanContext {
  packages: PackageSnapshots
  scope: NormalizedExoticSubdepsScope
  workspaceDirs: Set<string>
  peerCtx: PeerCheckContext
  isReplaced?: VerifyLockfileResolutionsOptions['isReplaced']
  violations: Map<string, ResolutionPolicyViolation>
  skippedReplaced: boolean
}

export function collectExoticSubdepViolations (
  lockfile: LockfileObject,
  isReplaced?: VerifyLockfileResolutionsOptions['isReplaced'],
  rawScope?: ExoticSubdepsScope
): ExoticSubdepScanResult {
  const packages = lockfile.packages
  if (!packages) return { violations: [], skippedReplaced: false }

  const scope = normalizeExoticSubdepsScope(rawScope)
  const workspaceDirs = collectWorkspaceDirectories(lockfile, scope)
  const ctx: ScanContext = {
    packages,
    scope,
    workspaceDirs,
    peerCtx: {
      packages,
      allowedPeerProviders: collectAllowedPeerProviders(lockfile, scope, workspaceDirs),
      dedupePeers: lockfile.settings?.dedupePeers === true,
    },
    isReplaced,
    violations: new Map<string, ResolutionPolicyViolation>(),
    skippedReplaced: false,
  }
  for (const [parentDepPath, parentSnapshot] of Object.entries(packages) as Array<[DepPath, PackageSnapshot]>) {
    scanParentSnapshot(ctx, parentDepPath, parentSnapshot)
  }
  return {
    violations: Array.from(ctx.violations.values()),
    skippedReplaced: ctx.skippedReplaced,
  }
}

function scanParentSnapshot (
  ctx: ScanContext,
  parentDepPath: DepPath,
  parentSnapshot: PackageSnapshot
): void {
  if (isWorkspaceSnapshot(parentSnapshot, ctx.workspaceDirs)) return
  const parentState = createParentPeerState(parentDepPath, parentSnapshot)
  scanDependencyMap(ctx, parentState, parentSnapshot.dependencies)
  scanDependencyMap(ctx, parentState, parentSnapshot.optionalDependencies)
}

function scanDependencyMap (
  ctx: ScanContext,
  parent: ParentPeerState,
  deps: ResolvedDependencies | undefined
): void {
  if (deps == null) return
  for (const [alias, ref] of Object.entries(deps)) {
    if (typeof ref === 'string') {
      scanDependencyEdge(ctx, parent, { alias, ref })
    }
  }
}

function scanDependencyEdge (
  ctx: ScanContext,
  parent: ParentPeerState,
  { alias, ref }: { alias: string, ref: string }
): void {
  const childDepPath = dp.refToRelative(ref, alias)
  if (childDepPath == null) return
  const childSnapshot = lookupPackageSnapshot(ctx.packages, childDepPath)
  const resolvedVia = exoticResolvedVia({ alias, childDepPath, snapshot: childSnapshot }, ctx.scope)
  if (resolvedVia == null) return
  if (isPeerSatisfactionEdge({ alias, childDepPath, childSnapshot }, parent, ctx.peerCtx)) return
  if (isEdgeReplaced(ctx.isReplaced, {
    parentDepPath: parent.parentDepPath,
    parentSnapshot: parent.parentSnapshot,
    childDepPath,
    childSnapshot,
  })) {
    ctx.skippedReplaced = true
    return
  }
  insertExoticViolation(ctx.violations, { alias, ref, childDepPath, childSnapshot, resolvedVia })
}

interface EdgeSnapshots {
  parentDepPath: DepPath
  parentSnapshot: PackageSnapshot
  childDepPath: DepPath
  childSnapshot: PackageSnapshot | undefined
}

function isEdgeReplaced (
  isReplaced: VerifyLockfileResolutionsOptions['isReplaced'],
  edge: EdgeSnapshots
): boolean {
  if (isReplaced == null) return false
  return snapshotIsReplaced(edge.parentDepPath, edge.parentSnapshot, isReplaced) ||
    snapshotIsReplaced(edge.childDepPath, edge.childSnapshot, isReplaced)
}

interface ViolationInput {
  alias: string
  ref: string
  childDepPath: DepPath
  childSnapshot: PackageSnapshot | undefined
  resolvedVia: string
}

function insertExoticViolation (
  violations: Map<string, ResolutionPolicyViolation>,
  { alias, ref, childDepPath, childSnapshot, resolvedVia }: ViolationInput
): void {
  const childInfo = childSnapshot != null
    ? nameVerFromPkgSnapshot(childDepPath, childSnapshot)
    : dp.parse(childDepPath)
  const name = childInfo.name || alias
  const version = childInfo.version || childInfo.nonSemverVersion || ref
  const resolution = (childSnapshot?.resolution ?? { tarball: childInfo.nonSemverVersion ?? ref }) as Resolution
  const dedupeKey = `${name}@${version}@${JSON.stringify(resolution)}`
  if (!violations.has(dedupeKey)) {
    violations.set(dedupeKey, {
      name,
      version,
      resolution,
      code: EXOTIC_SUBDEP_VIOLATION_CODE,
      reason: `(resolved via ${resolvedVia}) is not allowed in subdependencies when blockExoticSubdeps is enabled`,
    })
  }
}

interface KeyShape {
  name: string
  isRegistryKey: boolean
  isFileKey: boolean
  runtimeVersion?: string
  nonSemverVersion?: string
  scope: NormalizedExoticSubdepsScope
}

function exoticResolvedVia (
  edge: { alias: string, childDepPath: DepPath, snapshot: PackageSnapshot | undefined },
  scope: NormalizedExoticSubdepsScope
): string | undefined {
  const { alias, childDepPath, snapshot } = edge
  const bareDepPath = dp.removeSuffix(childDepPath) as DepPath
  const { name, nonSemverVersion } = dp.parse(bareDepPath)
  if (!name) return undefined
  const keyShape: KeyShape = {
    name,
    isRegistryKey: nonSemverVersion == null,
    isFileKey: nonSemverVersion?.startsWith('file:') === true,
    runtimeVersion: extractRuntimeVersion({ alias, name, nonSemverVersion, snapshot }),
    nonSemverVersion,
    scope,
  }
  if (snapshot?.resolution == null) {
    if (nonSemverVersion?.startsWith('runtime:') === true) return 'url'
    return (nonSemverVersion != null && !keyShape.isFileKey)
      ? classifyNonSemverLocator(nonSemverVersion)
      : undefined
  }
  return resolutionExoticResolvedVia(snapshot.resolution, keyShape)
}

function extractRuntimeVersion (opts: {
  alias: string
  name: string
  nonSemverVersion: string | undefined
  snapshot: PackageSnapshot | undefined
}): string | undefined {
  const { alias, name, nonSemverVersion, snapshot } = opts
  if (alias !== name || (name !== 'node' && name !== 'deno' && name !== 'bun')) return undefined
  if (nonSemverVersion == null || !nonSemverVersion.startsWith('runtime:')) return undefined
  const lockedVersion = nonSemverVersion.slice('runtime:'.length)
  if (semver.valid(lockedVersion) !== lockedVersion) return undefined
  const snapshotVersion = (snapshot as { version?: unknown } | undefined)?.version
  if (snapshotVersion != null && snapshotVersion !== lockedVersion) return undefined
  return lockedVersion
}

function resolutionExoticResolvedVia (resolution: unknown, keyShape: KeyShape): string | undefined {
  if (typeof resolution !== 'object' || resolution == null) {
    return keyShape.isRegistryKey ? undefined : classifyNonSemverLocator(keyShape.nonSemverVersion ?? '')
  }
  const { type, gitHosted, tarball, url, variants } = resolution as {
    type?: unknown
    gitHosted?: unknown
    tarball?: unknown
    url?: unknown
    variants?: unknown
  }
  if (typeof type === 'string') {
    return typedResolutionExoticVia(type, { url, variants }, keyShape)
  }
  if (type != null) return 'url'
  return tarballOrRegistryExoticResolvedVia(gitHosted, tarball, keyShape)
}

function typedResolutionExoticVia (
  type: string,
  fields: { url?: unknown, variants?: unknown },
  keyShape: KeyShape
): string | undefined {
  if (type === 'git') return 'git-repository'
  if (type === 'directory' || type.startsWith('custom:')) return undefined
  if (type === 'binary') {
    const allowed = keyShape.runtimeVersion != null &&
      typeof fields.url === 'string' &&
      isAllowedRuntimeBinaryUrl({
        runtimeName: keyShape.name,
        lockedVersion: keyShape.runtimeVersion,
        url: fields.url,
        scope: keyShape.scope,
      })
    return allowed ? undefined : 'url'
  }
  if (type === 'variations') return variationsExoticResolvedVia(fields.variants, keyShape)
  return 'url'
}

interface RuntimeBinaryCheck {
  runtimeName: string
  lockedVersion: string
  url: string
  scope: NormalizedExoticSubdepsScope
}

interface RuntimeArtifactTarget {
  runtimeName: string
  lockedVersion: string
  parsed: URL
}

function isAllowedRuntimeBinaryUrl ({ runtimeName, lockedVersion, url, scope }: RuntimeBinaryCheck): boolean {
  if (url.includes('\\') || hasUnsafeUrlPathSegments(url)) return false
  const parsed = parseHttpUrlWithoutCredentials(url)
  if (parsed == null) return false
  const target: RuntimeArtifactTarget = { runtimeName, lockedVersion, parsed }
  if (runtimeName === 'node') {
    return matchesAnyRuntimePrefix(target, [...DEFAULT_NODE_PREFIXES, ...scope.nodeMirrors])
  }
  if (runtimeName === 'deno') return matchesAnyRuntimePrefix(target, DEFAULT_DENO_PREFIXES)
  if (runtimeName === 'bun') return matchesAnyRuntimePrefix(target, DEFAULT_BUN_PREFIXES)
  return false
}

function parseHttpUrlWithoutCredentials (rawUrl: string): URL | undefined {
  try {
    const parsed = new URL(rawUrl)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return undefined
    if (parsed.username !== '' || parsed.password !== '') return undefined
    return parsed
  } catch {
    return undefined
  }
}

function matchesAnyRuntimePrefix (target: RuntimeArtifactTarget, prefixes: readonly string[]): boolean {
  for (const rawPrefix of prefixes) {
    if (urlMatchesRuntimePrefix(target, rawPrefix)) return true
  }
  return false
}

function urlMatchesRuntimePrefix (target: RuntimeArtifactTarget, rawPrefix: string): boolean {
  const prefix = parseHttpUrlWithoutCredentials(rawPrefix)
  if (prefix == null || !prefix.pathname.endsWith('/')) return false
  const { parsed } = target
  if (parsed.protocol !== prefix.protocol || parsed.hostname !== prefix.hostname || parsed.port !== prefix.port) {
    return false
  }
  if (!parsed.pathname.startsWith(prefix.pathname)) return false
  const relPath = parsed.pathname.slice(prefix.pathname.length)
  return runtimeArtifactSubpathMatches(target.runtimeName, target.lockedVersion, relPath)
}

function runtimeArtifactSubpathMatches (runtimeName: string, lockedVersion: string, relPath: string): boolean {
  const segments = relPath.split('/')
  if (segments.some((seg) => seg === '')) return false
  if (runtimeName === 'node') return nodeArtifactSubpathMatches(lockedVersion, segments)
  return zipRuntimeArtifactSubpathMatches(runtimeName, lockedVersion, segments)
}

function nodeArtifactSubpathMatches (lockedVersion: string, segments: string[]): boolean {
  const tail = segments.length === 3 ? segments.slice(1) : segments
  if (tail.length !== 2) return false
  const [releaseDir, fileName] = tail
  const validArchive = fileName.endsWith('.tar.gz') || fileName.endsWith('.zip')
  return releaseDir === `v${lockedVersion}` && fileName.startsWith(`node-v${lockedVersion}-`) && validArchive
}

function zipRuntimeArtifactSubpathMatches (runtimeName: string, lockedVersion: string, segments: string[]): boolean {
  if (segments.length !== 2) return false
  const [releaseDir, fileName] = segments
  if (!fileName.endsWith('.zip')) return false
  if (runtimeName === 'deno') return releaseDir === `v${lockedVersion}` && fileName.startsWith('deno-')
  if (runtimeName === 'bun') return releaseDir === `bun-v${lockedVersion}` && fileName.startsWith('bun-')
  return false
}

function hasUnsafeUrlPathSegments (url: string): boolean {
  const schemeIdx = url.indexOf('://')
  if (schemeIdx === -1) return true
  const afterScheme = url.slice(schemeIdx + 3)
  const slashIdx = afterScheme.indexOf('/')
  if (slashIdx === -1) return false
  const pathAndTail = afterScheme.slice(slashIdx)
  const queryIdx = pathAndTail.search(/[?#]/)
  const rawPath = queryIdx === -1 ? pathAndTail : pathAndTail.slice(0, queryIdx)
  if (ENCODED_PATH_ESCAPE_RE.test(rawPath)) return true
  return rawPath.split('/').some((seg) => seg === '.' || seg === '..')
}

function variationsExoticResolvedVia (variants: unknown, keyShape: KeyShape): string | undefined {
  if (!Array.isArray(variants) || variants.length === 0) return 'url'
  for (const variant of variants) {
    const inner = (variant as { resolution?: unknown })?.resolution
    if (inner == null) return 'url'
    const exoticVia = resolutionExoticResolvedVia(inner, keyShape)
    if (exoticVia != null) return exoticVia
  }
  return undefined
}

function tarballOrRegistryExoticResolvedVia (
  gitHosted: unknown,
  tarball: unknown,
  keyShape: KeyShape
): string | undefined {
  if (isGitHostedFlagOrUrl(gitHosted, tarball)) return 'git-repository'
  if (typeof tarball !== 'string' || tarball === '') {
    return keyShape.isRegistryKey ? undefined : classifyNonSemverLocator(keyShape.nonSemverVersion ?? '')
  }
  const isNonExoticTarball = (keyShape.isRegistryKey && /^https?:\/\//i.test(tarball)) ||
    (keyShape.isFileKey && tarball.startsWith('file:'))
  return isNonExoticTarball ? undefined : 'url'
}

function isGitHostedFlagOrUrl (gitHosted: unknown, tarball: unknown): boolean {
  if (gitHosted != null && (typeof gitHosted !== 'boolean' || gitHosted)) return true
  return typeof tarball === 'string' && isGitHostedTarballUrl(tarball)
}

const GIT_LOCATOR_RE = /^(?:git\+|github:|gitlab:|bitbucket:)/

function classifyNonSemverLocator (raw: string): string {
  return (GIT_LOCATOR_RE.test(raw) || isGitHostedTarballUrl(raw)) ? 'git-repository' : 'url'
}

function snapshotIsReplaced (
  depPath: DepPath,
  snapshot: PackageSnapshot | undefined,
  isReplaced: NonNullable<VerifyLockfileResolutionsOptions['isReplaced']>
): boolean {
  const info = snapshot != null ? nameVerFromPkgSnapshot(depPath, snapshot) : dp.parse(depPath)
  if (info.nonSemverVersion != null || !info.name || !info.version) return false
  return isReplaced(info.name, info.version, {
    registryName: info.registryName,
  })
}
