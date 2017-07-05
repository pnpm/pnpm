export type Dependencies = {
  [name: string]: string
}

export type PackageBin = string | {[name: string]: string}

export type Package = {
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
  scripts?: {
    [name: string]: string
  },
  config?: Object,
}
