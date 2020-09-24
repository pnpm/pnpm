import { WANTED_LOCKFILE } from '@pnpm/constants'
import linkBins, { WarnFunction } from '@pnpm/link-bins'
import {
  Lockfile,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import lockfileWalker, { LockfileWalkerStep } from '@pnpm/lockfile-walker'
import logger, { globalWarn } from '@pnpm/logger'
import matcher from '@pnpm/matcher'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import symlinkDependency from '@pnpm/symlink-dependency'
import { HoistedDependencies } from '@pnpm/types'
import * as dp from 'dependency-path'
import path = require('path')
import R = require('ramda')

export default async function hoistByLockfile (
  opts: {
    lockfile: Lockfile
    lockfileDir: string
    privateHoistPattern: string[]
    privateHoistedModulesDir: string
    publicHoistPattern: string[]
    publicHoistedModulesDir: string
    virtualStoreDir: string
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
    },
    ...await getDependencies(step, 0),
  ]

  const getAliasHoistType = createGetAliasHoistType(opts.publicHoistPattern, opts.privateHoistPattern)

  const hoistedDependencies = await hoistGraph(deps, opts.lockfile.importers['.']?.specifiers ?? {}, {
    getAliasHoistType,
  })

  await symlinkHoistedDependencies(hoistedDependencies, {
    lockfile: opts.lockfile,
    lockfileDir: opts.lockfileDir,
    privateHoistedModulesDir: opts.privateHoistedModulesDir,
    publicHoistedModulesDir: opts.publicHoistedModulesDir,
    virtualStoreDir: opts.virtualStoreDir,
  })

  // Here we only link the bins of the privately hoisted modules.
  // The bins of the publicly hoisted modules will be linked together with
  // the bins of the project's direct dependencies.
  // This is possible because the publicly hoisted modules
  // are in the same directory as the regular dependencies.
  await linkAllBins(opts.privateHoistedModulesDir)

  return hoistedDependencies
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
    logger.info({ message, prefix: path.join(modulesDir, '../..') })
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
  depth: number
): Promise<Dependency[]> {
  const deps: Dependency[] = []
  const nextSteps: LockfileWalkerStep[] = []
  for (const { pkgSnapshot, depPath, next } of step.dependencies) {
    const allDeps: Record<string, string> = {
      ...pkgSnapshot.dependencies,
      ...pkgSnapshot.optionalDependencies,
    }
    deps.push({
      children: Object.entries(allDeps).reduce((children, [alias, ref]) => {
        children[alias] = dp.refToRelative(ref, alias)
        return children
      }, {}),
      depPath,
      depth,
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
      nextSteps.map((nextStep) => getDependencies(nextStep, depth + 1))
    )
  ).reduce((acc, deps) => [...acc, ...deps], deps)
}

export interface Dependency {
  children: {[alias: string]: string}
  depPath: string
  depth: number
}

async function hoistGraph (
  depNodes: Dependency[],
  currentSpecifiers: {[alias: string]: string},
  opts: {
    getAliasHoistType: GetAliasHoistType
  }
): Promise<HoistedDependencies> {
  const hoistedAliases = new Set(R.keys(currentSpecifiers))
  const hoistedDependencies: HoistedDependencies = {}

  depNodes
    // sort by depth and then alphabetically
    .sort((a, b) => {
      const depthDiff = a.depth - b.depth
      return depthDiff === 0 ? a.depPath.localeCompare(b.depPath) : depthDiff
    })
    // build the alias map and the id map
    .forEach((depNode) => {
      for (const [childAlias, childPath] of Object.entries(depNode.children)) {
        const hoist = opts.getAliasHoistType(childAlias)
        if (!hoist) continue
        // if this alias has already been taken, skip it
        if (hoistedAliases.has(childAlias)) {
          continue
        }
        hoistedAliases.add(childAlias)
        if (!hoistedDependencies[childPath]) {
          hoistedDependencies[childPath] = {}
        }
        hoistedDependencies[childPath][childAlias] = hoist
      }
    })

  return hoistedDependencies
}

async function symlinkHoistedDependencies (
  hoistedDependencies: HoistedDependencies,
  opts: {
    lockfile: Lockfile
    lockfileDir: string
    privateHoistedModulesDir: string
    publicHoistedModulesDir: string
    virtualStoreDir: string
  }
) {
  await Promise.all(
    Object.entries(hoistedDependencies)
      .map(async ([depPath, pkgAliases]) => {
        const pkgSnapshot = opts.lockfile.packages![depPath]
        if (!pkgSnapshot) {
          globalWarn(`Failed to find "${depPath}" in lockfile during hoisting. ` +
            `Next aliases will not be hoisted: ${Object.keys(pkgAliases).join(', ')}`)
          return
        }
        const pkgName = nameVerFromPkgSnapshot(depPath, pkgSnapshot).name
        const modules = path.join(opts.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDir), 'node_modules')
        const depLocation = path.join(modules, pkgName)
        await Promise.all(Object.entries(pkgAliases).map(async ([pkgAlias, hoistType]) => {
          const targetDir = hoistType === 'public'
            ? opts.publicHoistedModulesDir : opts.privateHoistedModulesDir
          await symlinkDependency(depLocation, targetDir, pkgAlias)
        }))
      }
      ))
}
