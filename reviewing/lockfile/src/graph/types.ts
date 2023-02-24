export const enum NodeKind {
  Importer,
  Package,
  Missing
}

interface Node {
  key: string
  children: Set<PackageNode | ImporterNode>
  missing: Set<string>
  links: Set<PackageNode | ImporterNode>
  parents: Set<PackageNode | ImporterNode>
  kind: NodeKind
}

export type ImporterNode = Node

export interface PackageNode extends ImporterNode {
  host?: string
  name?: string
  version?: string
  peersSuffix?: string
}

export type ImporterPath = [ImporterNode, ...Array<PackageNode | string>]

export type Importers = Map<string, ImporterNode>
export type Packages = Map<string, PackageNode>

export interface Graph {
  importerIds: string[]
  packageKeys: string[]
  getImporter: (importerId: string) => ImporterNode | undefined
  getPackage: (packageKey: string) => PackageNode | undefined
  whoImportThisPackage: (packageKey: string) => string[][] | undefined
}

export interface DependencyPath {
  host: string | undefined
  isAbsolute: boolean
  name?: string | undefined
  peersSuffix?: string | undefined
  version?: string | undefined
}

export interface GraphBuilderContext<
  Importer,
  Package extends { dependencies?: Record<string, string>, optionalDependencies?: Record<string, string> },
  Lockfile extends { importers: Record<string, Importer>, packages?: Record<string, Package> }
> {
  parse: (rawLockfile: any) => Lockfile // eslint-disable-line
  entryNodes: (importer: Importer) => string[]
  nextPackages: (nowPackage: Package) => string[]
  refToRelative: (reference: string, pkgName: string) => string | null
  parseDependencyPath: (dependencyPath: string) => DependencyPath
}

export interface LockfileWalkerStep {
  dependencies: Array<{
    depPath: string
    next: () => LockfileWalkerStep
  }>
  links: string[]
  missing: string[]
}
