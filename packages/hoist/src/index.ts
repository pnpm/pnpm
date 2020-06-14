import { WANTED_LOCKFILE } from '@pnpm/constants'
import linkBins, { WarnFunction } from '@pnpm/link-bins'
import {
  Lockfile,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import lockfileWalker, { LockfileWalkerStep } from '@pnpm/lockfile-walker'
import logger from '@pnpm/logger'
import matcher from '@pnpm/matcher'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import symlinkDependency from '@pnpm/symlink-dependency'
import { Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import path = require('path')
import R = require('ramda')

export default async function hoistByLockfile (
  opts: {
    lockfile: Lockfile,
    lockfileDir: string,
    privateHoistPattern: string[],
    privateHoistDir: string,
    publicHoistPattern: string[],
    publicHoistDir: string,
    registries: Registries,
    virtualStoreDir: string,
  }
) {
  if (!opts.lockfile.packages) return {
    hoistedDeps: {},
    publiclyHoistedAliases: new Set<string>(),
  }

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

  const getAliasHoistType = createGetAliasHoistType(opts.publicHoistPattern, opts.privateHoistPattern)

  const { hoistedDeps, publiclyHoistedAliases } = await hoistGraph(deps, opts.lockfile.importers['.']?.specifiers ?? {}, {
    dryRun: false,
    getAliasHoistType,
    privateHoistDir: opts.privateHoistDir,
    publicHoistDir: opts.publicHoistDir,
  })

  await linkAllBins(opts.privateHoistDir)
  if (publiclyHoistedAliases.size) {
    await linkAllBins(opts.publicHoistDir)
  }

  return { hoistedDeps, publiclyHoistedAliases }
}

type GetAliasHoistType = (alias: string) => 'private' | 'public' | false

function createGetAliasHoistType (
  publicHoistPattern: string[],
  privateHoistPattern: string[]
): GetAliasHoistType {
  const publicMatcher = matcher(publicHoistPattern)
  const privateMatcher = matcher(privateHoistPattern)
  return (alias: string) => {
    if (publicMatcher(alias)) return 'public'
    if (privateMatcher(alias)) return 'private'
    return false
  }
}

async function linkAllBins (modulesDir: string) {
  const bin = path.join(modulesDir, '.bin')
  const warn: WarnFunction = (message, code) => {
    if (code === 'BINARIES_CONFLICT') return
    logger.warn({ message, prefix: path.join(modulesDir, '../..') })
  }
  try {
    await linkBins(modulesDir, bin, { allowExoticManifests: true, warn })
  } catch (err) {
    // Some packages generate their commands with lifecycle hooks.
    // At this stage, such commands are not generated yet.
    // For now, we don't hoist such generated commands.
    // Related issue: https://github.com/pnpm/pnpm/issues/2071
  }
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
    getAliasHoistType: GetAliasHoistType,
    privateHoistDir: string,
    publicHoistDir: string,
    dryRun: boolean,
  }
): Promise<{
  hoistedDeps: Record<string, string[]>,
  publiclyHoistedAliases: Set<string>,
}> {
  const hoistedAliasesSet = new Set(R.keys(currentSpecifiers))
  const hoistedDeps: {[depPath: string]: string[]} = {}
  const publiclyHoistedAliases = new Set<string>()

  await Promise.all(depNodes
    // sort by depth and then alphabetically
    .sort((a, b) => {
      const depthDiff = a.depth - b.depth
      return depthDiff === 0 ? a.depPath.localeCompare(b.depPath) : depthDiff
    })
    // build the alias map and the id map
    .map((depNode) => {
      for (const childAlias of Object.keys(depNode.children)) {
        const hoist = opts.getAliasHoistType(childAlias)
        if (!hoist) continue
        // if this alias has already been taken, skip it
        if (hoistedAliasesSet.has(childAlias)) {
          continue
        }
        hoistedAliasesSet.add(childAlias)
        const childPath = depNode.children[childAlias]
        if (!hoistedDeps[childPath]) {
          hoistedDeps[childPath] = []
        }
        hoistedDeps[childPath].push(childAlias)
        if (hoist === 'public') {
          publiclyHoistedAliases.add(childAlias)
        }
      }
      return depNode
    })
    .map(async (depNode) => {
      const pkgAliases = hoistedDeps[depNode.depPath]
      if (!pkgAliases) {
        return
      }
      // TODO when putting logs back in for hoisted packages, you've to put back the condition inside the map,
      // TODO look how it is done in linkPackages
      if (!opts.dryRun) {
        await Promise.all(pkgAliases.map(async (pkgAlias) => {
          if (publiclyHoistedAliases.has(pkgAlias)) {
            await symlinkDependency(depNode.location, opts.publicHoistDir, pkgAlias)
          } else {
            await symlinkDependency(depNode.location, opts.privateHoistDir, pkgAlias)
          }
        }))
      }
    }))

  return { hoistedDeps, publiclyHoistedAliases }
}
