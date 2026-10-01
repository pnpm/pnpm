import type { GetAuthHeader } from '@pnpm/fetching.types'
import type {
  DirectoryResolution,
  LatestInfo,
  LatestQuery,
  PkgResolutionId,
  PreferredVersions,
  Resolution,
  ResolveOptions,
  ResolveResult,
  TarballResolution,
  WantedDependency,
  WorkspacePackages,
} from '@pnpm/resolving.resolver-base'
import type {
  DependencyManifest,
  PackageVersionPolicy,
  RangeSpecStyle,
  RegistriesByScope,
  TrustPolicy,
} from '@pnpm/types'

import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import type { pickPackage, PickPackageOptions } from './pickPackage.js'

export interface NpmResolveResult extends ResolveResult {
  latest?: string
  manifest: DependencyManifest
  resolution: TarballResolution
  resolvedVia: 'npm-registry'
}

export interface JsrResolveResult extends ResolveResult {
  alias: string
  manifest: DependencyManifest
  resolution: TarballResolution
  resolvedVia: 'jsr-registry'
}

export interface NamedRegistryResolveResult extends ResolveResult {
  alias: string
  /** The named-registry alias that was matched, e.g. `gh` or a user-defined name. */
  registryName: string
  manifest: DependencyManifest
  resolution: TarballResolution
  resolvedVia: 'named-registry'
}

export interface WorkspaceResolveResult extends ResolveResult {
  manifest: DependencyManifest
  resolution: DirectoryResolution
  resolvedVia: 'workspace'
}

export type NpmResolver = (
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: ResolveFromNpmOptions
) => Promise<NpmResolveResult | JsrResolveResult | NamedRegistryResolveResult | WorkspaceResolveResult | null>

export type ResolveLatestFromNpmStyle = (
  query: LatestQuery,
  opts: ResolveOptions
) => Promise<LatestInfo | undefined>

export interface ResolveFromNpmContext {
  pickPackage: (spec: RegistryPackageSpec, opts: PickPackageOptions) => ReturnType<typeof pickPackage>
  getAuthHeaderValueByURI: GetAuthHeader
  registriesByScope: RegistriesByScope
  registriesByPrefix: Record<string, string>
  namedRegistryNames: ReadonlySet<string>
  saveWorkspaceProtocol?: boolean | 'rolling'
  /**
   * The `minimumReleaseAgeIgnoreMissingTime` opt-in, reaching the trust
   * check as well as the version pick: both read the same per-version
   * `time`, so a registry that strips it takes both down together.
   */
  ignoreMissingTimeField?: boolean
  peekManifestFromStore?: (opts: {
    id: PkgResolutionId
    integrity: string
    name?: string
    version?: string
  }) => Promise<DependencyManifest | undefined>
  /** Deduplicates the held-back-update warning per `(name, picked, preferred)`. */
  warnedHeldBackUpdates: Set<string>
  /** Deduplicates the trust-downgrade fallback warning per `name@picked`. */
  warnedTrustDowngradeFallbacks: Set<string>
}

export type ResolveFromNpmOptions = {
  alwaysTryWorkspacePackages?: boolean
  defaultTag?: string
  publishedBy?: Date
  fallbackPublishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
  pickLowestVersion?: boolean
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: PackageVersionPolicy
  trustPolicyIgnoreAfter?: number
  dryRun?: boolean
  lockfileDir?: string
  preferredVersions?: PreferredVersions
  preferWorkspacePackages?: boolean
  update?: false | 'compatible' | 'latest'
  updatePatches?: boolean
  updateRequested?: boolean
  updateChecksums?: boolean
  injectWorkspacePackages?: boolean
  calcSpecifier?: boolean
  rangeSpecStyle?: RangeSpecStyle
  currentPkg?: {
    id: PkgResolutionId
    name?: string
    version?: string
    resolution: Resolution
    publishedAt?: string
  }
} & ({
  projectDir?: string
  workspacePackages?: undefined
} | {
  projectDir: string
  workspacePackages: WorkspacePackages
})

/** A registry lookup of one parsed spec, shared by every registry resolver. */
export interface RegistrySpecRequest {
  ctx: ResolveFromNpmContext
  wantedDependency: WantedDependency & { optional?: boolean }
  opts: Omit<ResolveFromNpmOptions, 'registry'>
  spec: RegistryPackageSpec
  registry: string
}
