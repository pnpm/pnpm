import { WANTED_LOCKFILE } from '@pnpm/constants'
import linkBins, { WarnFunction } from '@pnpm/link-bins'
import {
  Lockfile,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import lockfileWalker, { LockfileWalkerStep } from '@pnpm/lockfile-walker'
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
    lockfile: Lockfile,
    lockfileDir: string,
    modulesDir: string,
    registries: Registries,
    virtualStoreDir: string,
  }
) {
  if (!opts.lockfile.packages) return {}

  const { directDeps, step } = lockfileWalker(
    opts.lockfile,
    Object.keys(opts.lockfile.importers)
  )
  const deps = [
    {
      children: directDeps
        .reduce((acc, { alias, depPath }) => {
          if (!acc[alias]) {
            acc[alias] = depPath
          }
          return acc
        }, {}),
      depPath: '',
      depth: -1,
      location: '',
    },
    ...await getDependencies(
      step,
      0,
      {
        lockfileDir: opts.lockfileDir,
        registries: opts.registries,
        virtualStoreDir: opts.virtualStoreDir,
      }
    ),
  ]

  const aliasesByDependencyPath = await hoistGraph(deps, opts.lockfile.importers['.']?.specifiers ?? {}, {
    dryRun: false,
    match,
    modulesDir: opts.modulesDir,
  })

  const bin = path.join(opts.modulesDir, '.bin')
  const warn: WarnFunction = (message, code) => {
    if (code === 'BINARIES_CONFLICT') return
    logger.warn({ message, prefix: path.join(opts.modulesDir, '../..') })
  }
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
  step: LockfileWalkerStep,
  depth: number,
  opts: {
    registries: Registries,
    lockfileDir: string,
    virtualStoreDir: string,
  }
): Promise<Dependency[]> {
  const deps: Dependency[] = []
  const nextSteps: LockfileWalkerStep[] = []
  for (const { pkgSnapshot, depPath, next } of step.dependencies) {
    const pkgName = nameVerFromPkgSnapshot(depPath, pkgSnapshot).name
    const modules = path.join(opts.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDir), 'node_modules')
    const allDeps = {
      ...pkgSnapshot.dependencies,
      ...pkgSnapshot.optionalDependencies,
    }
    deps.push({
      children: Object.keys(allDeps).reduce((children, alias) => {
        children[alias] = dp.refToRelative(allDeps[alias], alias)
        return children
      }, {}),
      depPath,
      depth,
      location: path.join(modules, pkgName),
    })

    nextSteps.push(next())
  }

  for (const depPath of step.missing) {
    // It might make sense to fail if the depPath is not in the skipped list from .modules.yaml
    // However, the skipped list currently contains package IDs, not dep paths.
    logger.debug({ message: `No entry for "${depPath}" in ${WANTED_LOCKFILE}` })
  }

  return (
    await Promise.all(
      nextSteps.map((nextStep) => getDependencies(nextStep, depth + 1, opts))
    )
  ).reduce((acc, deps) => [...acc, ...deps], deps)
}

export interface Dependency {
  location: string,
  children: {[alias: string]: string},
  depPath: string,
  depth: number,
}

async function hoistGraph (
  depNodes: Dependency[],
  currentSpecifiers: {[alias: string]: string},
  opts: {
    match: (dependencyName: string) => boolean,
    modulesDir: string,
    dryRun: boolean,
  }
): Promise<{[alias: string]: string[]}> {
  const hoistedAliases = new Set(R.keys(currentSpecifiers))
  const aliasesByDependencyPath: {[depPath: string]: string[]} = {}

  await Promise.all(depNodes
    // sort by depth and then alphabetically
    .sort((a, b) => {
      const depthDiff = a.depth - b.depth
      return depthDiff === 0 ? a.depPath.localeCompare(b.depPath) : depthDiff
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
      const pkgAliases = aliasesByDependencyPath[depNode.depPath]
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
