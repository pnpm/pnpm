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
    node?: string
    patch?: string
  }
}

export interface PublishConfig extends Record<string, unknown> {
  directory?: string
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
  bin?: PackageBin
  description?: string
  directories?: {
    bin?: string
  }
  files?: string[]
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
  scripts?: PackageScripts
  config?: object
  engines?: {
    node?: string
    npm?: string
    pnpm?: string
  }
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

export interface ProjectManifest extends BaseManifest {
  packageManager?: string
  workspaces?: string[]
  pnpm?: {
    neverBuiltDependencies?: string[]
    onlyBuiltDependencies?: string[]
    onlyBuiltDependenciesFile?: string
    overrides?: Record<string, string>
    packageExtensions?: Record<string, PackageExtension>
    ignoredOptionalDependencies?: string[]
    peerDependencyRules?: PeerDependencyRules
    allowedDeprecatedVersions?: AllowedDeprecatedVersions
    allowNonAppliedPatches?: boolean
    patchedDependencies?: Record<string, string>
    updateConfig?: {
      ignoreDependencies?: string[]
    }
    auditConfig?: {
      ignoreCves?: string[]
    }
    requiredScripts?: string[]
    supportedArchitectures?: SupportedArchitectures
  }
  private?: boolean
  resolutions?: Record<string, string>
}

export interface PackageManifest extends DependencyManifest {
  deprecated?: string
}

export interface SupportedArchitectures {
  os?: string[]
  cpu?: string[]
  libc?: string[]
}
