import type {
  DependencyManifest,
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
  injectWorkspacePackages?: boolean
  calcSpecifier?: boolean
  pinnedVersion?: PinnedVersion
  currentPkg?: {
    id: PkgResolutionId
    name?: string
    version?: string
    resolution: Resolution
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
