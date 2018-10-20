export interface Dependencies {
  [name: string]: string,
}

export type PackageBin = string | {[commandName: string]: string}

export type PackageScripts = {
  [name: string]: string,
} & {
  prepublish?: string,
  prepare?: string,
  prepublishOnly?: string,
  prepack?: string,
  postpack?: string,
  publish?: string,
  postpublish?: string,
  preinstall?: string,
  install?: string,
  postinstall?: string,
  preuninstall?: string,
  uninstall?: string,
  postuninstall?: string,
  preversion?: string,
  version?: string,
  postversion?: string,
  pretest?: string,
  test?: string,
  posttest?: string,
  prestop?: string,
  stop?: string,
  poststop?: string,
  prestart?: string,
  start?: string,
  poststart?: string,
  prerestart?: string,
  restart?: string,
  postrestart?: string,
  preshrinkwrap?: string,
  shrinkwrap?: string,
  postshrinkwrap?: string,
}

export interface PackageJson {
  name: string,
  version: string,
  private?: boolean,
  bin?: PackageBin,
  directories?: {
    bin?: string,
  },
  dependencies?: Dependencies,
  devDependencies?: Dependencies,
  optionalDependencies?: Dependencies,
  peerDependencies?: Dependencies,
  bundleDependencies?: string[],
  bundledDependencies?: string[],
  scripts?: PackageScripts,
  config?: object,
  engines?: {
    node?: string,
    npm?: string,
  },
  cpu?: string[],
  os?: string[],
}

// Most of the fields in PackageManifest are also in PackageJson
// except the `deprecated` field
export interface PackageManifest {
  name: string,
  version: string,
  bin?: PackageBin,
  directories?: {
    bin?: string,
  },
  dependencies?: Dependencies,
  devDependencies?: Dependencies,
  optionalDependencies?: Dependencies,
  peerDependencies?: Dependencies,
  bundleDependencies?: string[],
  bundledDependencies?: string[],
  engines?: {
    node?: string,
    npm?: string,
  },
  cpu?: string[],
  os?: string[],
  deprecated?: string,
}
