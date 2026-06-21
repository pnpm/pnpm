import type {
  DependencyManifest,
  PackageManifest,
  PackageVersionPolicy,
  PinnedVersion,
  PkgResolutionId,
  ProjectRootDir,
  SupportedArchitectures,
  TrustPolicy,
} from '@pnpm/types'

export { type PkgResolutionId }

/**
 * tarball hosted remotely
 */
export interface TarballResolution {
  type?: undefined
  tarball: string
  integrity?: string
  path?: string
  /**
   * True for tarballs sourced from a git host (codeload.github.com /
   * gitlab.com / bitbucket.org). Such tarballs need preparation
   * (preparePackage / packlist) on extraction, and their cached content
   * depends on whether build scripts ran, so they're addressed by
   * gitHostedStoreIndexKey rather than the integrity-based key.
   */
  gitHosted?: boolean
}

export interface BinaryResolution {
  type: 'binary'
  archive: 'tarball' | 'zip'
  url: string
  integrity: string
  bin: string | Record<string, string>
  prefix?: string
}

/**
 * directory on a file system
 */
export interface DirectoryResolution {
  type: 'directory'
  directory: string
}

export interface GitResolution {
  commit: string
  repo: string
  path?: string
  type: 'git'
}

export interface CustomResolution {
  type: `custom:${string}` // e.g., 'custom:cdn', 'custom:artifactory'
  [key: string]: unknown
}

export interface PlatformAssetTarget {
  os: string
  cpu: string
  libc?: 'musl'
}

export interface PlatformAssetResolution {
  resolution: AtomicResolution
  targets: PlatformAssetTarget[]
}

export type AtomicResolution =
  | TarballResolution
  | DirectoryResolution
  | GitResolution
  | BinaryResolution
  | CustomResolution

export interface VariationsResolution {
  type: 'variations'
  variants: PlatformAssetResolution[]
}

export type Resolution = AtomicResolution | VariationsResolution

const GIT_HOSTED_TARBALL_HOSTS = [
  'https://codeload.github.com/',
  'https://bitbucket.org/',
  'https://gitlab.com/',
]

/**
 * A tarball URL is git-hosted when it points at a known git provider's archive
 * endpoint (GitHub/GitLab/Bitbucket). Schemes and hostnames are case-insensitive,
 * so the match runs against a lowercased copy: a tampered
 * `https://CODELOAD.GITHUB.COM/...` must not slip past as a registry-trusted
 * tarball. Only the lowercased copy is inspected; the original URL is never rewritten.
 */
export function isGitHostedTarballUrl (url: string): boolean {
  const lowerUrl = url.toLowerCase()
  return GIT_HOSTED_TARBALL_HOSTS.some((host) => lowerUrl.startsWith(host)) && lowerUrl.includes('tar.gz')
}

/**
 * The kind of source a resolution points at — used by fetcher selection (`pickFetcher`).
 * `'custom'` covers `custom:*` resolution types (served by custom fetchers) and any other
 * non-built-in shape.
 */
export type ResolutionKind =
  | 'localTarball'
  | 'gitHostedTarball'
  | 'remoteTarball'
  | 'directory'
  | 'git'
  | 'binary'
  | 'custom'

/**
 * Classifies a resolution by the kind of source it points at. A tarball-shaped
 * resolution (no `type`) is a `remoteTarball` unless its URL marks it as local
 * (`file:`) or git-hosted. A canonical registry entry whose tarball URL was omitted
 * (reconstructed later from name/version/registry) still classifies as `remoteTarball`.
 */
export function classifyResolution (resolution: Resolution): ResolutionKind {
  // `== null` so a tarball entry deserialized from YAML with `type: null` is treated the
  // same as an absent `type`, instead of falling through to `custom`.
  if (resolution.type == null) {
    if (resolution.tarball?.startsWith('file:')) return 'localTarball'
    if (resolution.gitHosted === true || (resolution.tarball != null && isGitHostedTarballUrl(resolution.tarball))) {
      return 'gitHostedTarball'
    }
    return 'remoteTarball'
  }
  switch (resolution.type) {
    case 'directory':
    case 'git':
    case 'binary':
      return resolution.type
    default:
      return 'custom'
  }
}

/**
 * Outcome of asking a `ResolutionVerifier` whether a (name, version,
 * resolution) entry from a lockfile is acceptable under whatever policies
 * the resolver chain has been configured with. Resolvers that don't have
 * an opinion on a given resolution should return `{ ok: true }`.
 */
export type ResolutionVerification =
  | { ok: true }
  | { ok: false, code: string, reason: string }

/**
 * Optional companion to a resolver factory.
 *
 * `verify` inspects the `resolution` shape to decide whether the entry
 * is within its protocol; for entries outside its protocol it should
 * return `{ ok: true }`. The install side fans out across the verifier
 * list rather than asking a combinator to dispatch.
 *
 * `policy` and `canTrustPastCheck` describe the verifier's cache
 * contract. Policies from every active verifier are merged into a
 * single shared bag stored alongside the lockfile hash; the
 * install-side verification cache reads them to decide if a previous
 * run on the same lockfile is still trustworthy under today's policy
 * without re-issuing the registry round-trips that `verify` would.
 * Verifiers that check the same logical policy (e.g. minimumReleaseAge
 * across registries) name it the same and share the cache slot.
 */
export interface ResolutionVerifier {
  /**
   * `ctx.nonSemverVersion` is set when the lockfile entry is keyed by a
   * non-semver reference (URL tarball, git, etc.) rather than a registry
   * `name@version`. Verifiers that only police registry entries use it to
   * skip deliberate non-registry deps, which can still carry a semver
   * `version` copied from the resolved manifest.
   */
  verify: (resolution: Resolution, ctx: { name: string, version: string, nonSemverVersion?: string }) => Promise<ResolutionVerification>
  /**
   * Snapshot of the policy fields this verifier enforces. Merged with
   * every other active verifier's `policy` into the cache record. A
   * field shared across verifiers (same key) should carry the same
   * value; if it doesn't, the last verifier in the list wins.
   */
  policy: Record<string, unknown>
  /**
   * Returns true when the previously cached policy (the merged snapshot
   * from the last successful run) can be trusted to still satisfy what
   * this verifier currently demands. Reads whichever fields the
   * verifier owns; missing or non-conforming values (e.g. an older
   * record shape) should return false. A loosened policy can trust a
   * stricter cached run; a tightened policy cannot.
   */
  canTrustPastCheck: (cachedPolicy: Record<string, unknown>) => boolean
}

/**
 * A `ResolutionVerifier`'s rejection materialized for one (name,
 * version, resolution) entry. The install side aggregates these across
 * every active verifier on the freshly-resolved tree and either prompts
 * the user, persists them (e.g. into `minimumReleaseAgeExclude`), or
 * aborts. Code is the verifier-defined error code
 * (`MINIMUM_RELEASE_AGE_VIOLATION`, `TRUST_DOWNGRADE`, etc.) — the
 * install command filters by code to decide downstream UX. Lifted here
 * (rather than in deps-installer) so both deps-resolver and
 * deps-installer can share one shape; future resolver packages plug in
 * without needing the deps-installer dependency.
 */
export interface ResolutionPolicyViolation {
  name: string
  version: string
  resolution: Resolution
  code: string
  reason: string
}

/** Concrete platform selector used when picking a variant from a VariationsResolution. */
export interface PlatformSelector {
  os: string
  cpu: string
  /** Name of the libc family requested. Omit (or leave `null`) for the default (glibc on Linux, n/a elsewhere). */
  libc?: string | null
}

/**
 * Resolve a {@link PlatformSelector} from the user's supportedArchitectures config
 * and the host's own platform/arch/libc. When `supportedArchitectures.xxx` is set
 * and its first entry is not `"current"`, that entry wins; otherwise the host's
 * value is used. Additional entries beyond the first are ignored — variant
 * selection picks exactly one (os, cpu, libc) triplet per install.
 */
export function resolvePlatformSelector (
  supportedArchitectures: SupportedArchitectures | undefined,
  host: { platform: string, arch: string, libc: string | null | undefined }
): PlatformSelector {
  return {
    os: pickFirstNonCurrent(supportedArchitectures?.os) ?? host.platform,
    cpu: pickFirstNonCurrent(supportedArchitectures?.cpu) ?? host.arch,
    libc: pickFirstNonCurrent(supportedArchitectures?.libc) ?? host.libc,
  }
}

/**
 * Pick the variant whose target matches the given selector, or `undefined` if
 * none does. A variant with no `libc` represents the "default" build — glibc on
 * Linux, irrelevant on macOS/Windows. A non-default libc (e.g. `musl`) is a
 * separate, non-interchangeable artifact; an exact libc match is required in
 * that case so the glibc/default variant doesn't silently win (its `target.libc`
 * is nullish).
 */
export function selectPlatformVariant (
  variants: PlatformAssetResolution[],
  selector: PlatformSelector
): PlatformAssetResolution | undefined {
  return variants.find((variant) => variant.targets.some((target) =>
    target.os === selector.os &&
    target.cpu === selector.cpu &&
    libcMatches(target.libc, selector.libc)
  ))
}

function libcMatches (variantLibc: string | undefined, requestedLibc: string | null | undefined): boolean {
  if (requestedLibc == null || requestedLibc === 'glibc') {
    return variantLibc == null
  }
  return variantLibc === requestedLibc
}

function pickFirstNonCurrent (requirements: string[] | undefined): string | undefined {
  if (requirements?.length && requirements[0] !== 'current') {
    return requirements[0]
  }
  return undefined
}

export interface ResolveResult {
  id: PkgResolutionId
  latest?: string
  publishedAt?: string
  manifest?: DependencyManifest
  resolution: Resolution
  resolvedVia: string
  normalizedBareSpecifier?: string
  alias?: string
  /**
   * Set when the resolver picked this version despite a policy
   * violation (e.g. immature relative to `publishedBy`, trust
   * downgrade detected by `failIfTrustDowngraded`). The resolver
   * already has the metadata it needs to decide, so reporting inline
   * here avoids the install layer having to re-scan the tree and
   * re-fetch the same metadata. The deps-resolver aggregates these
   * across every resolve call into a single set the install command
   * can react to.
   *
   * `resolution` on the violation is the same `resolution` field
   * above — supplied for symmetry with `ResolutionPolicyViolation`
   * entries that flow out of `verifyLockfileResolutions` for
   * lockfile-only paths.
   */
  policyViolation?: ResolutionPolicyViolation
}

export interface WorkspacePackage {
  rootDir: ProjectRootDir
  manifest: DependencyManifest
}

export type WorkspacePackagesByVersion = Map<string, WorkspacePackage>

export type WorkspacePackages = Map<string, WorkspacePackagesByVersion>

// This weight is set for selectors that are used on direct dependencies.
// It is important to give a bigger weight to direct dependencies.
export const DIRECT_DEP_SELECTOR_WEIGHT = 1000

// This weight is set for concrete versions of dependencies preexisting in the
// wanted lockfile. When adding a dependency, prefer existing versions first.
//
// This needs to be a higher weight than DIRECT_DEP_SELECTOR_WEIGHT since direct
// dependency specifiers can match a range of versions. Versions on the registry
// not present in the lockfile should be considered at a lower weight than
// matching pre-existing versions. If this is not the case, pnpm could suddenly
// introduce a new version in the lockfile when an existing version works.
export const EXISTING_VERSION_SELECTOR_WEIGHT = 1_000_000

export type VersionSelectorType = 'version' | 'range' | 'tag'

export interface VersionSelectors {
  [selector: string]: VersionSelectorWithWeight | VersionSelectorType
}

export interface VersionSelectorWithWeight {
  selectorType: VersionSelectorType
  weight: number
}

export interface PreferredVersions {
  [packageName: string]: VersionSelectors
}

export interface ResolveOptions {
  alwaysTryWorkspacePackages?: boolean
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: PackageVersionPolicy
  trustPolicyIgnoreAfter?: number
  defaultTag?: string
  pickLowestVersion?: boolean
  publishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
  projectDir: string
  lockfileDir: string
  preferredVersions: PreferredVersions
  preferWorkspacePackages?: boolean
  workspacePackages?: WorkspacePackages
  update?: false | 'compatible' | 'latest'
  updateChecksums?: boolean
  injectWorkspacePackages?: boolean
  calcSpecifier?: boolean
  pinnedVersion?: PinnedVersion
  currentPkg?: {
    id: PkgResolutionId
    name?: string
    version?: string
    resolution: Resolution
    publishedAt?: string
  }
}

export type WantedDependency = {
  injected?: boolean
  prevSpecifier?: string
} & ({
  alias?: string
  bareSpecifier: string
} | {
  alias: string
  bareSpecifier?: string
})

export type ResolveFunction = (wantedDependency: WantedDependency & { optional?: boolean }, opts: ResolveOptions) => Promise<ResolveResult>

/**
 * Input to a resolver's `resolveLatest` function. The resolver decides
 * whether it owns this dep purely from `wantedDependency` (its alias and
 * manifest specifier) — the lockfile-resolved ref is the caller's
 * concern, not the resolver's.
 */
export interface LatestQuery {
  wantedDependency: WantedDependency
  compatible?: boolean
}

/**
 * Result of a resolver's `resolveLatest` call.
 *
 * - `undefined` means "this resolver does not handle this dep — try
 *   the next one".
 * - An object (even without a `latestManifest`) means "I claim this
 *   dep, but I can't tell you what's latest" (e.g. policy blocked,
 *   network unavailable, no concept of latest for this protocol).
 *   The caller still surfaces a ref-mismatch report if the lockfile
 *   shifted.
 */
export interface LatestInfo {
  latestManifest?: PackageManifest
}

export type ResolveLatestFunction = (
  query: LatestQuery,
  opts: ResolveOptions
) => Promise<LatestInfo | undefined>
