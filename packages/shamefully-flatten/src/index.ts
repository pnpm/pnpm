import logger from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import {
  nameVerFromPkgSnapshot,
  PackageSnapshots,
  Shrinkwrap,
} from '@pnpm/shrinkwrap-utils'
import symlinkDependency from '@pnpm/symlink-dependency'
import * as dp from 'dependency-path'
import path = require('path')
import R = require('ramda')

export async function shamefullyFlattenByShrinkwrap (
  shr: Shrinkwrap,
  importerId: string,
  opts: {
    defaultRegistry: string,
    modulesDir: string,
    prefix: string,
    virtualStoreDir: string,
  },
) {
  if (!shr.packages) return {}

  const shrImporter = shr.importers[importerId]

  const entryNodes = R.toPairs({
    ...shrImporter.devDependencies,
    ...shrImporter.dependencies,
    ...shrImporter.optionalDependencies,
  })
  .map((pair) => dp.refToRelative(pair[1], pair[0]))
  .filter((nodeId) => nodeId !== null) as string[]

  const deps = getDependencies(shr.packages, entryNodes, new Set(), 0, {
    prefix: opts.prefix,
    registry: opts.defaultRegistry,
    virtualStoreDir: opts.virtualStoreDir,
  })

  return shamefullyFlattenGraph(deps, shrImporter.specifiers, {
    dryRun: false,
    modulesDir: opts.modulesDir,
  })
}

function getDependencies (
  pkgSnapshots: PackageSnapshots,
  depRelPaths: string[],
  walked: Set<string>,
  depth: number,
  opts: {
    registry: string,
    prefix: string,
    virtualStoreDir: string,
  },
): Dependency[] {
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
      logger.debug({ message: `No entry for "${depRelPath}" in shrinkwrap.yaml` })
      continue
    }

    const absolutePath = dp.resolve(opts.registry, depRelPath)
    const pkgName = nameVerFromPkgSnapshot(depRelPath, pkgSnapshot).name
    const modules = path.join(opts.virtualStoreDir, `.${pkgIdToFilename(absolutePath, opts.prefix)}`, 'node_modules')
    const peripheralLocation = path.join(modules, pkgName)
    const allDeps = {
      ...pkgSnapshot.dependencies,
      ...pkgSnapshot.optionalDependencies,
    }
    deps.push({
      absolutePath,
      children: R.keys(allDeps).reduce((children, alias) => {
        children[alias] = dp.refToAbsolute(allDeps[alias], alias, opts.registry)
        return children
      }, {}),
      depth,
      name: pkgName,
      peripheralLocation,
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
    ...getDependencies(pkgSnapshots, nextDepRelPaths, walked, depth + 1, opts),
  ]
}

export interface Dependency {
  name: string,
  // TODO: support linking from central location.
  //   it is needed to support --independent-leaves
  // centralLocation: string,
  peripheralLocation: string,
  children: {[alias: string]: string},
  // independent: boolean,
  depth: number,
  absolutePath: string,
}

export default async function shamefullyFlattenGraph (
  depNodes: Dependency[],
  currentSpecifiers: {[alias: string]: string},
  opts: {
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
      for (const childAlias of R.keys(depNode.children)) {
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
          await symlinkDependency(depNode.peripheralLocation, opts.modulesDir, pkgAlias)
        }))
      }
    }))

  return aliasesByDependencyPath
}
