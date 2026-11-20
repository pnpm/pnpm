import path from 'node:path'

import { makeNodePackageMapOption } from '@pnpm/exec.lifecycle'
import { PACKAGE_MAP_FILENAME, removePackageMap, writePackageMap, writePackageMapFromDependenciesGraph } from '@pnpm/lockfile.to-pnp'

import type { HeadlessContext, HeadlessDepGraph } from './context.js'

/** Writes or removes the package map and returns whether it was written. */
export async function updatePackageMap (ctx: HeadlessContext, depGraph: HeadlessDepGraph): Promise<boolean> {
  const { opts } = ctx
  // See the matching gate in `deps-installer`.
  const shouldWritePackageMap = opts.nodeExperimentalPackageMap === true && opts.enableModulesDir !== false && opts.nodeLinker !== 'pnp' && !opts.virtualStoreOnly
  if (shouldWritePackageMap) {
    await writePackageMapOfProjects(ctx, depGraph)
  } else if (opts.enableModulesDir !== false && !opts.virtualStoreOnly) {
    await removePackageMap(ctx.rootModulesDir)
  }
  return shouldWritePackageMap
}

async function writePackageMapOfProjects (ctx: HeadlessContext, depGraph: HeadlessDepGraph): Promise<void> {
  const { opts } = ctx
  // Omit the importer self-mapping when a project has no name: the map keys
  // dependencies by package name, so falling back to the importer id (`.` or
  // a path) would emit a non-package-name key. Matches pacquet.
  const importerNames = Object.fromEntries(
    ctx.selectedProjects.map(({ manifest, id }) => [id, manifest.name])
  )
  if (opts.nodeLinker === 'hoisted') {
    await writePackageMapFromDependenciesGraph({
      directDependenciesByImporterId: depGraph.directDependenciesByImporterId,
      graph: depGraph.graph,
      importerNames,
      lockfile: depGraph.filteredLockfile,
      lockfileDir: ctx.lockfileDir,
      packageMapType: opts.nodePackageMapType,
      packageIdStrategy: 'path',
      rootModulesDir: ctx.rootModulesDir,
    })
    return
  }
  await writePackageMap(depGraph.filteredLockfile, {
    importerNames,
    lockfileDir: ctx.lockfileDir,
    locationByDepPath: Object.fromEntries(
      Object.values(depGraph.graph).map((node) => [node.depPath, node.dir])
    ),
    packageMapType: opts.nodePackageMapType,
    rootModulesDir: ctx.rootModulesDir,
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
  })
}

export function addPackageMapOption (
  extraEnv: Record<string, string> | undefined,
  rootModulesDir: string
): Record<string, string> {
  return {
    ...extraEnv,
    ...makeNodePackageMapOption(path.join(rootModulesDir, PACKAGE_MAP_FILENAME), extraEnv),
  }
}
