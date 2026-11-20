import * as dp from '@pnpm/deps.path'
import type { RequestPackageFunction } from '@pnpm/store.controller-types'
import type {
  DepPath,
  PackageManifest,
  PackageVersionPolicy,
  ReadPackageHook,
  TrustPolicy,
} from '@pnpm/types'
import { clone } from 'ramda'

import type { FastOverride } from './fastRewritePackages.js'

export interface ResolverPolicyOptions {
  publishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: PackageVersionPolicy
  trustPolicyIgnoreAfter?: number
}

type PackageResponse = Awaited<ReturnType<RequestPackageFunction>>

export type RegistryResolution = PackageResponse['body']['resolution']

/** The manifest a moved package has at its new version, and where it is served from. */
export interface ResolvedManifest {
  manifest: PackageManifest
  resolution: RegistryResolution
}

type ManifestRequestOptions = ResolverPolicyOptions & {
  lockfileDir: string
  readPackageHook?: ReadPackageHook
  requestPackage: RequestPackageFunction
}

/**
 * The new version's manifest for every package the replacements move, keyed
 * by name. `null` when one of them cannot be linked without a resolution.
 */
export async function resolveNewManifests (
  overrides: FastOverride[],
  replacements: Map<DepPath, DepPath>,
  opts: ManifestRequestOptions
): Promise<Map<string, ResolvedManifest> | null> {
  const changedNames = new Set(
    [...replacements]
      .filter(([oldDepPath, newDepPath]) => oldDepPath !== newDepPath)
      .map(([oldDepPath]) => dp.parse(oldDepPath).name!)
  )
  const results = await Promise.all(overrides.map(async ({ name, newVersion }) => {
    if (newVersion == null) return null
    if (!changedNames.has(name)) return null
    return resolveNewManifest({ name, version: newVersion }, opts)
  }))
  if (results.some((result) => result === undefined)) return null
  return new Map(results
    .filter((result) => result != null)
    .map(({ name, ...value }) => [name, value]))
}

/** The manifest of `expected`, or `undefined` when the fast path cannot link it. */
async function resolveNewManifest (
  expected: { name: string, version: string },
  opts: ManifestRequestOptions
): Promise<(ResolvedManifest & { name: string }) | undefined> {
  const response = await opts.requestPackage({
    alias: expected.name,
    bareSpecifier: expected.version,
  }, {
    downloadPriority: 0,
    lockfileDir: opts.lockfileDir,
    preferredVersions: Object.create(null),
    projectDir: opts.lockfileDir,
    publishedBy: opts.publishedBy,
    publishedByExclude: opts.publishedByExclude,
    skipFetch: true,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    update: false,
  })
  if (!isPlainRegistryResponse(response)) return undefined
  const rawManifest = response.body.manifest!
  if (!manifestFitsFastPath(rawManifest, expected)) return undefined
  const manifest = opts.readPackageHook == null
    ? rawManifest
    : await opts.readPackageHook(clone(rawManifest))
  // pacquet validates after its manifest hook, so a hook that introduces
  // any of these must send both stacks down the same fallback.
  if (!manifestFitsFastPath(manifest, expected)) return undefined
  return {
    name: expected.name,
    manifest,
    resolution: response.body.resolution,
  }
}

/** Whether the response is a registry tarball with an integrity, as the lockfile records one. */
function isPlainRegistryResponse (response: PackageResponse): boolean {
  if (
    response.body.isLocal ||
    response.body.manifest == null ||
    response.body.policyViolation != null ||
    response.body.resolvedVia !== 'npm-registry' ||
    response.resolutionNeedsFetch === true
  ) {
    return false
  }
  return 'integrity' in response.body.resolution &&
    typeof response.body.resolution.integrity === 'string' &&
    response.body.resolution.type == null
}

/**
 * Whether the fast path can link this manifest in place of a resolution:
 * it is the requested version and declares nothing the fast path does not
 * reproduce.
 */
function manifestFitsFastPath (manifest: PackageManifest, expected: { name: string, version: string }): boolean {
  return manifest.name === expected.name &&
    manifest.version === expected.version &&
    manifest.deprecated == null &&
    !hasInvalidManifestMaps(manifest) &&
    !hasPeerDependencies(manifest) &&
    manifest.engines?.runtime == null &&
    manifest.bundledDependencies == null &&
    manifest.bundleDependencies == null
}

function hasInvalidManifestMaps (manifest: PackageManifest): boolean {
  return [
    manifest.dependencies,
    manifest.optionalDependencies,
    manifest.peerDependencies,
    manifest.peerDependenciesMeta,
    manifest.engines,
  ].some((value) =>
    value != null &&
    (typeof value !== 'object' || Array.isArray(value))
  )
}

function hasPeerDependencies (manifest: PackageManifest): boolean {
  return Object.keys(manifest.peerDependencies ?? {}).length > 0 ||
    Object.keys(manifest.peerDependenciesMeta ?? {}).length > 0
}
