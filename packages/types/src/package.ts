export interface Dependencies {
  [name: string]: string
}

export type PackageBin = string | {[commandName: string]: string}

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

interface BaseManifest {
  name?: string
  version?: string
  bin?: PackageBin
  description?: string
  directories?: {
    bin?: string
  }
  dependencies?: Dependencies
  devDependencies?: Dependencies
  optionalDependencies?: Dependencies
  peerDependencies?: Dependencies
  peerDependenciesMeta?: PeerDependenciesMeta
  bundleDependencies?: string[]
  bundledDependencies?: string[]
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
  main?: string
  module?: string
  typings?: string
  types?: string
  publishConfig?: Record<string, unknown>
}

export type DependencyManifest = BaseManifest & Required<Pick<BaseManifest, 'name' | 'version'>>

export type ProjectManifest = BaseManifest & {
  pnpm?: {
    overrides?: Record<string, string>
  }
  private?: boolean
  resolutions?: Record<string, string>
}

export type PackageManifest = DependencyManifest & {
  deprecated?: string
}
