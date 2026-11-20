import type { ProjectSnapshot } from '@pnpm/lockfile.fs'
import normalizePath from 'normalize-path'

import {
  joinPath,
  type LinkBase,
  relativePath,
  resolveLinkTarget,
  resolvePath,
  sortedEntries,
} from './packageMapPaths.js'
import {
  addDependencyLocation,
  addExternalLinkPackage,
  addPackage,
  addPackageLocation,
  buildPackageMap,
  createPackageMapState,
  type PackageMapState,
} from './packageMapState.js'
import type { DependenciesGraphPackageMapOptions, PackageMap, PackageMapGraphNode } from './packageMapTypes.js'

interface GraphPackageMapContext {
  opts: DependenciesGraphPackageMapOptions
  packageIdsByGraphKey: Map<string, string>
  state: PackageMapState
}

interface LinkedDependencies {
  base: LinkBase & { modulesDir?: string }
  groups: Array<Record<string, string> | undefined>
}

export function dependenciesGraphToPackageMap (
  opts: DependenciesGraphPackageMapOptions
): PackageMap {
  const ctx: GraphPackageMapContext = {
    opts,
    packageIdsByGraphKey: new Map<string, string>(),
    state: createPackageMapState(opts),
  }
  const graphEntries = sortedEntries(Object.entries(opts.graph))
  for (const [graphKey, node] of graphEntries) {
    registerGraphNode(ctx, graphKey, node)
  }
  for (const [importerId, importer] of sortedEntries(Object.entries(opts.lockfile.importers))) {
    addImporter(ctx, importerId, importer)
  }
  for (const [graphKey, node] of graphEntries) {
    addGraphPackage(ctx, graphKey, node)
  }
  return buildPackageMap(ctx.state)
}

function registerGraphNode (ctx: GraphPackageMapContext, graphKey: string, node: PackageMapGraphNode): void {
  const packageId = graphNodePackageId(node, ctx.opts)
  ctx.packageIdsByGraphKey.set(graphKey, packageId)
  addPackageLocation(ctx.state, node.dir, { name: node.name, id: packageId })
}

function addImporter (ctx: GraphPackageMapContext, importerId: string, importer: ProjectSnapshot): void {
  const { opts } = ctx
  const importerDir = resolvePath(opts.lockfileDir, importerId)
  const importerPackageId = graphPackageId(importerDir, opts)
  const dependencies = new Map<string, string>()
  const importerName = opts.importerNames[importerId]
  if (importerName) {
    dependencies.set(importerName, importerPackageId)
  }
  addGraphDependencies(ctx, dependencies, opts.directDependenciesByImporterId[importerId])
  addLinkedDependencies(ctx, dependencies, {
    base: { importerId, modulesDir: ctx.state.isLoose ? joinPath(importerDir, 'node_modules') : undefined },
    groups: [importer.dependencies, importer.optionalDependencies, importer.devDependencies],
  })
  addPackage(ctx.state, { id: importerPackageId, dir: importerDir, dependencies })
}

function addGraphPackage (ctx: GraphPackageMapContext, graphKey: string, node: PackageMapGraphNode): void {
  const packageId = ctx.packageIdsByGraphKey.get(graphKey)!
  const dependencies = new Map<string, string>([[node.name, packageId]])
  addGraphDependencies(ctx, dependencies, node.children)

  const pkgSnapshot = ctx.opts.lockfile.packages?.[node.depPath]
  if (pkgSnapshot) {
    addLinkedDependencies(ctx, dependencies, {
      base: { modulesDir: ctx.state.isLoose ? joinPath(node.dir, 'node_modules') : undefined, packageDir: node.dir },
      groups: [pkgSnapshot.dependencies, pkgSnapshot.optionalDependencies],
    })
  }

  addPackage(ctx.state, { id: packageId, dir: node.dir, dependencies })
}

function addGraphDependencies (
  ctx: GraphPackageMapContext,
  dependencies: Map<string, string>,
  deps: Record<string, string> | undefined
): void {
  for (const [alias, graphKey] of sortedEntries(Object.entries(deps ?? {}))) {
    const packageId = ctx.packageIdsByGraphKey.get(graphKey)
    if (packageId) dependencies.set(alias, packageId)
  }
}

function addLinkedDependencies (
  ctx: GraphPackageMapContext,
  dependencies: Map<string, string>,
  linked: LinkedDependencies
): void {
  const { opts, state } = ctx
  const { base } = linked
  for (const [alias, ref] of linked.groups.flatMap((deps) => sortedEntries(Object.entries(deps ?? {})))) {
    if (!ref.startsWith('link:')) continue
    const target = resolveLinkTarget(opts.lockfileDir, base, ref)
    const targetId = opts.packageIdStrategy === 'path'
      ? graphPackageId(target.dir, opts)
      : target.id
    addExternalLinkPackage(state, {
      ...target,
      id: targetId,
    })
    dependencies.set(alias, targetId)
    if (base.modulesDir) {
      addDependencyLocation(state, base.modulesDir, { name: alias, id: targetId })
    }
  }
}

function graphNodePackageId (node: PackageMapGraphNode, opts: DependenciesGraphPackageMapOptions): string {
  if (opts.packageIdStrategy === 'depPath') return node.depPath
  return graphPackageId(node.dir, opts)
}

function graphPackageId (packageDir: string, opts: Pick<DependenciesGraphPackageMapOptions, 'rootModulesDir'>): string {
  const relativeId = relativePath(opts.rootModulesDir, packageDir)
  if (relativeId == null) return `link:${normalizePath(packageDir)}`
  return relativeId === '..' ? '.' : relativeId
}
