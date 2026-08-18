import type { RegistriesByScope, RegistryDeclaration } from './misc.js'
import type { VersioningSettings } from './versioning.js'

export type Dependencies = Record<string, string>

export type PackageBin = string | { [commandName: string]: string }

export type PackageScripts = {
  [name: string]: string
} & {
  prepublish?: string
  prepare?: string
  prepublishOnly?: string
  prepack?: string
  postpack?: string
  publish?: string
  postpublish?: string
  preinstall?: string
  install?: string
  postinstall?: string
  preuninstall?: string
  uninstall?: string
  postuninstall?: string
  preversion?: string
  version?: string
  postversion?: string
  pretest?: string
  test?: string
  posttest?: string
  prestop?: string
  stop?: string
  poststop?: string
  prestart?: string
  start?: string
  poststart?: string
  prerestart?: string
  restart?: string
  postrestart?: string
  preshrinkwrap?: string
  shrinkwrap?: string
  postshrinkwrap?: string
}

export interface PeerDependenciesMeta {
  [dependencyName: string]: {
    optional?: boolean
  }
}

export interface DependenciesMeta {
  [dependencyName: string]: {
    injected?: boolean
    patch?: string
  }
}

export const RUNTIME_NAMES = ['node', 'deno', 'bun'] as const

export type RuntimeName = typeof RUNTIME_NAMES[number]

export function isRuntimeAlias (alias: string): alias is RuntimeName {
  return (RUNTIME_NAMES as readonly string[]).includes(alias)
}

export interface EngineDependency {
  name: string
  version?: string
  onFail?: 'ignore' | 'warn' | 'error' | 'download'
}

type DevEngineKey = 'os' | 'cpu' | 'libc' | 'runtime' | 'packageManager'

export type DevEngines = Partial<Record<DevEngineKey, EngineDependency | EngineDependency[]>>

export interface PublishConfig extends Record<string, unknown> {
  access?: 'public' | 'restricted'
  directory?: string
  /**
   * Publishes the package under a different name than the one its manifest
   * carries in the workspace — for a project whose name is already taken by a
   * sibling. Only the published artifact is renamed; dependents, the lockfile,
   * and release tooling keep addressing the project by its manifest name.
   */
  name?: string
  linkDirectory?: boolean
  executableFiles?: string[]
  registry?: string
}

type Version = string
type Pattern = string
export interface TypesVersions {
  [version: Version]: {
    [pattern: Pattern]: string[]
  }
}

export interface BaseManifest {
  name?: string
  version?: string
  type?: string
  bin?: PackageBin
  description?: string
  directories?: {
    bin?: string
  }
  files?: string[]
  funding?: string
  dependencies?: Dependencies
  devDependencies?: Dependencies
  optionalDependencies?: Dependencies
  peerDependencies?: Dependencies
  peerDependenciesMeta?: PeerDependenciesMeta
  dependenciesMeta?: DependenciesMeta
  bundleDependencies?: string[] | boolean
  bundledDependencies?: string[] | boolean
  homepage?: string
  repository?: string | { url: string }
  bugs?: string | {
    url?: string
    email?: string
  }
  scripts?: PackageScripts
  config?: Record<string, unknown>
  engines?: {
    node?: string
    npm?: string
    pnpm?: string
  } & Pick<DevEngines, 'runtime'>
  devEngines?: DevEngines
  cpu?: string[]
  os?: string[]
  libc?: string[]
  main?: string
  module?: string
  typings?: string
  types?: string
  publishConfig?: PublishConfig
  typesVersions?: TypesVersions
  readme?: string
  keywords?: string[]
  author?: string
  license?: string
  exports?: Record<string, string>
  imports?: Record<string, unknown>
}

export interface DependencyManifest extends BaseManifest {
  name: string
  version: string
}

export type PackageExtension = Pick<BaseManifest, 'dependencies' | 'optionalDependencies' | 'peerDependencies' | 'peerDependenciesMeta'>

export interface PeerDependencyRules {
  ignoreMissing?: string[]
  allowAny?: string[]
  allowedVersions?: Record<string, string>
}

export type AllowedDeprecatedVersions = Record<string, string>

type VersionWithIntegrity = string

/**
 * Old format (inline integrity in pnpm-workspace.yaml):
 *   "@my-org/cfg": "1.2.0+sha512-XYZ"
 *   or { tarball: "...", integrity: "1.2.0+sha512-XYZ" }
 *
 * New format (plain specifiers in pnpm-workspace.yaml, integrity in pnpm-lock.yaml):
 *   "@my-org/cfg": "^1.2.0"
 */
export type ConfigDependencies = Record<string, VersionWithIntegrity | {
  tarball?: string
  integrity: VersionWithIntegrity
}>

/**
 * Clean specifiers for configDependencies in pnpm-workspace.yaml (new format).
 * Integrity info is stored in pnpm-lock.yaml instead.
 */
export type ConfigDependencySpecifiers = Record<string, string>

export type AuditLevel = 'info' | 'low' | 'moderate' | 'high' | 'critical'

/**
 * @deprecated Use {@link AuditSettings} instead. Kept for backward
 * compatibility until the next major version.
 */
export interface AuditConfig {
  ignoreGhsas?: string[]
}

export interface AuditSettings {
  /**
   * The minimum vulnerability severity `pnpm audit` reports on. Supersedes the
   * top-level `auditLevel` setting.
   */
  level?: AuditLevel
  /**
   * GHSA IDs that `pnpm audit` should ignore. Supersedes
   * `auditConfig.ignoreGhsas`.
   */
  ignore?: string[]
}

export interface UpdateSettings {
  /**
   * Dependency name patterns that `pnpm update` and `pnpm outdated` should skip.
   */
  ignoreDeps?: string[]
  /**
   * Generate a changeset for the updated production dependencies by default,
   * as if `pnpm update` were run with `--changeset`.
   */
  changeset?: boolean
  /**
   * Whether `pnpm outdated` and `pnpm update` should also look at the GitHub
   * Actions referenced by the workflow files. Opt-in: neither command reads
   * them unless this is set to `true` or `--include-github-actions` is passed.
   */
  githubActions?: boolean
  /**
   * The base URL of the GitHub server that hosts the repositories of the
   * GitHub Actions referenced by the workflow files (for example, a GitHub
   * Enterprise Server). When not set, the `GITHUB_SERVER_URL` environment
   * variable is used, falling back to https://github.com.
   */
  githubActionsServer?: string
}

export interface PnpmSettings {
  npmrcAuthFile?: string
  /**
   * The registries the project declares, keyed by registry URL, so that a
   * registry's layout, the scopes routed to it, and the bare-specifier prefix
   * it answers to are all stated once, in one place.
   *
   * A map whose values are plain strings is the older `scope: url` shape and
   * is read as one.
   */
  registries?: Record<string, RegistryDeclaration> | RegistriesByScope
  /**
   * @deprecated Give the registry a `prefix` in
   * {@link PnpmSettings.registries} instead. Kept working until the next major
   * version.
   */
  namedRegistries?: Record<string, string>
  configDependencies?: ConfigDependencies
  allowBuilds?: Record<string, boolean | string>
  overrides?: Record<string, string>
  packageExtensions?: Record<string, PackageExtension>
  ignoredOptionalDependencies?: string[]
  peerDependencyRules?: PeerDependencyRules
  allowedDeprecatedVersions?: AllowedDeprecatedVersions
  allowUnusedPatches?: boolean
  patchedDependencies?: Record<string, string>
  update?: UpdateSettings
  /**
   * @deprecated Use {@link PnpmSettings.update} instead. Kept for backward
   * compatibility until the next major version.
   */
  updateConfig?: {
    changeset?: boolean
    ignoreDependencies?: string[]
    githubActions?: boolean
    githubActionsServer?: string
  }
  audit?: AuditSettings
  /**
   * @deprecated Use {@link PnpmSettings.audit} instead. Kept for backward
   * compatibility until the next major version.
   */
  auditConfig?: AuditConfig
  requiredScripts?: string[]
  supportedArchitectures?: SupportedArchitectures
  nodeDownloadMirrors?: Record<string, string>
  httpProxy?: string
  httpsProxy?: string
  noProxy?: string | boolean
  pnprServer?: string
  versioning?: VersioningSettings
  /**
   * Where the virtual store lives, and therefore who shares it: one store
   * per machine (`global`) or one per project (`project`).
   *
   * The canonical spelling of {@link PnpmSettings.enableGlobalVirtualStore}.
   * When both are set, this one wins.
   */
  virtualStoreType?: VirtualStoreType
  /**
   * The boolean spelling of {@link PnpmSettings.virtualStoreType}.
   */
  enableGlobalVirtualStore?: boolean
}

export type VirtualStoreType = 'global' | 'project'

export interface ProjectManifest extends BaseManifest {
  packageManager?: string
  workspaces?: string[] // TODO: add Record<string, string> to represent npm (to be compatible with @npm/types)
  private?: boolean
  resolutions?: Record<string, string>
}

export interface PackageManifest extends DependencyManifest {
  deprecated?: string
}

/**
 * Subset of package.json fields cached in the store index.
 * Used for bin linking, build scripts, runtime selection, and dependency resolution.
 */
export type BundledManifest = Pick<
  BaseManifest,
| 'bin'
| 'bundledDependencies'
| 'bundleDependencies'
| 'cpu'
| 'dependencies'
| 'devDependencies'
| 'directories'
| 'engines'
| 'libc'
| 'name'
| 'optionalDependencies'
| 'os'
| 'peerDependencies'
| 'peerDependenciesMeta'
| 'scripts'
| 'version'
>

export interface SupportedArchitectures {
  os?: string[]
  cpu?: string[]
  libc?: string[]
}
