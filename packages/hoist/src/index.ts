import { WANTED_LOCKFILE } from '@pnpm/constants'
import linkBins from '@pnpm/link-bins'
import {
  Lockfile,
  nameVerFromPkgSnapshot,
  packageIsIndependent,
  PackageSnapshots,
} from '@pnpm/lockfile-utils'
import logger from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import symlinkDependency from '@pnpm/symlink-dependency'
import { Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import path = require('path')
import R = require('ramda')

export default async function hoistByLockfile (
  match: (dependencyName: string) => boolean,
  opts: {
    getIndependentPackageLocation?: (packageId: string, packageName: string) => Promise<string>,
    lockfile: Lockfile,
    lockfileDir: string,
    modulesDir: string,
    registries: Registries,
    virtualStoreDir: string,
  },
) {
  if (!opts.lockfile.packages) return {}

  const entryNodes = new Set<string>()
  for (let importerId of Object.keys(opts.lockfile.importers).sort()) {
    const lockfileImporter = opts.lockfile.importers[importerId]
    ; (
      R.toPairs({
        ...lockfileImporter.devDependencies,
        ...lockfileImporter.dependencies,
        ...lockfileImporter.optionalDependencies,
      })
      .map((pair) => dp.refToRelative(pair[1], pair[0]))
      .filter((nodeId) => nodeId !== null) as string[]
    ).forEach((relativePath) => { entryNodes.add(relativePath) })
  }

  const deps = await getDependencies(opts.lockfile.packages, Array.from(entryNodes.values()), new Set(), 0, {
    getIndependentPackageLocation: opts.getIndependentPackageLocation,
    lockfileDir: opts.lockfileDir,
    registries: opts.registries,
    virtualStoreDir: opts.virtualStoreDir,
  })

  const aliasesByDependencyPath = await hoistGraph(deps, opts.lockfile.importers['.'].specifiers, {
    dryRun: false,
    match,
    modulesDir: opts.modulesDir,
  })

  const bin = path.join(opts.modulesDir, '.bin')
  const warn = (message: string) => logger.warn({ message, prefix: path.join(opts.modulesDir, '../..') })
  try {
    await linkBins(opts.modulesDir, bin, { allowExoticManifests: true, warn })
  } catch (err) {
    // Some packages generate their commands with lifecycle hooks.
    // At this stage, such commands are not generated yet.
    // For now, we don't hoist such generated commands.
    // Related issue: https://github.com/pnpm/pnpm/issues/2071
  }

  return aliasesByDependencyPath
}

async function getDependencies (
  pkgSnapshots: PackageSnapshots,
  depRelPaths: string[],
  walked: Set<string>,
  depth: number,
  opts: {
    getIndependentPackageLocation?: (packageId: string, packageName: string) => Promise<string>,
    registries: Registries,
    lockfileDir: string,
    virtualStoreDir: string,
  },
): Promise<Dependency[]> {
  if (depRelPaths.length === 0) return []

  const deps: Dependency[] = []
  let nextDepRelPaths = [] as string[]
  for (const depRelPath of depRelPaths) {
    if (walked.has(depRelPath)) continue
    walked.add(depRelPath)

    const pkgSnapshot = pkgSnapshots[depRelPath]
    if (!pkgSnapshot) {
      if (depRelPath.startsWith('link:')) continue

      // It might make sense to fail if the depPath is not in the skipped list from .modules.yaml
      // However, the skipped list currently contains package IDs, not dep paths.
      logger.debug({ message: `No entry for "${depRelPath}" in ${WANTED_LOCKFILE}` })
      continue
    }

    const absolutePath = dp.resolve(opts.registries, depRelPath)
    const pkgName = nameVerFromPkgSnapshot(depRelPath, pkgSnapshot).name
    const modules = path.join(opts.virtualStoreDir, pkgIdToFilename(absolutePath, opts.lockfileDir), 'node_modules')
    const independent = opts.getIndependentPackageLocation && packageIsIndependent(pkgSnapshot)
    const allDeps = {
      ...pkgSnapshot.dependencies,
      ...pkgSnapshot.optionalDependencies,
    }
    deps.push({
      absolutePath,
      children: Object.keys(allDeps).reduce((children, alias) => {
        children[alias] = dp.refToAbsolute(allDeps[alias], alias, opts.registries)
        return children
      }, {}),
      depth,
      location: !independent
        ? path.join(modules, pkgName)
        : await opts.getIndependentPackageLocation!(pkgSnapshot.id || absolutePath, pkgName),
      name: pkgName,
    })

    nextDepRelPaths = [
      ...nextDepRelPaths,
      ...R.toPairs({
        ...pkgSnapshot.dependencies,
        ...pkgSnapshot.optionalDependencies,
      })
      .map((pair) => dp.refToRelative(pair[1], pair[0]))
      .filter((nodeId) => nodeId !== null) as string[],
    ]
  }

  return [
    ...deps,
    ...await getDependencies(pkgSnapshots, nextDepRelPaths, walked, depth + 1, opts),
  ]
}

export interface Dependency {
  name: string,
  location: string,
  children: {[alias: string]: string},
  depth: number,
  absolutePath: string,
}

async function hoistGraph (
  depNodes: Dependency[],
  currentSpecifiers: {[alias: string]: string},
  opts: {
    match: (dependencyName: string) => boolean,
    modulesDir: string,
    dryRun: boolean,
  },
): Promise<{[alias: string]: string[]}> {
  const hoistedAliases = new Set(R.keys(currentSpecifiers))
  const aliasesByDependencyPath: {[depPath: string]: string[]} = {}

  await Promise.all(depNodes
    // sort by depth and then alphabetically
    .sort((a, b) => {
      const depthDiff = a.depth - b.depth
      return depthDiff === 0 ? a.name.localeCompare(b.name) : depthDiff
    })
    // build the alias map and the id map
    .map((depNode) => {
      for (const childAlias of Object.keys(depNode.children)) {
        if (!opts.match(childAlias)) continue
        // if this alias has already been taken, skip it
        if (hoistedAliases.has(childAlias)) {
          continue
        }
        hoistedAliases.add(childAlias)
        const childPath = depNode.children[childAlias]
        if (!aliasesByDependencyPath[childPath]) {
          aliasesByDependencyPath[childPath] = []
        }
        aliasesByDependencyPath[childPath].push(childAlias)
      }
      return depNode
    })
    .map(async (depNode) => {
      const pkgAliases = aliasesByDependencyPath[depNode.absolutePath]
      if (!pkgAliases) {
        return
      }
      // TODO when putting logs back in for hoisted packages, you've to put back the condition inside the map,
      // TODO look how it is done in linkPackages
      if (!opts.dryRun) {
        await Promise.all(pkgAliases.map(async (pkgAlias) => {
          await symlinkDependency(depNode.location, opts.modulesDir, pkgAlias)
        }))
      }
    }))

  return aliasesByDependencyPath
}
