import fs from 'fs'
import path from 'path'
import { linkLogger } from '@pnpm/core-loggers'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { linkBinsOfPkgsByAliases, type WarnFunction } from '@pnpm/link-bins'
import {
  type Lockfile,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import { lockfileWalker, type LockfileWalkerStep } from '@pnpm/lockfile-walker'
import { logger } from '@pnpm/logger'
import { createMatcher } from '@pnpm/matcher'
import { type ProjectManifest, type HoistedDependencies } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import * as dp from '@pnpm/dependency-path'
import isSubdir from 'is-subdir'
import mapObjIndexed from 'ramda/src/mapObjIndexed'
import resolveLinkTarget from 'resolve-link-target'
import symlinkDir from 'symlink-dir'

const hoistLogger = logger('hoist')

interface Project {
  binsDir: string
  buildIndex: number
  manifest: ProjectManifest
  modulesDir: string
  id: string
  pruneDirectDependencies?: boolean
  rootDir: string
}

export interface HoistOpts extends GetHoistedDependenciesOpts {
  extraNodePath?: string[]
  preferSymlinkedExecutables?: boolean
  virtualStoreDir: string
  allProjects: Record<string, Project>
}

export async function hoist (opts: HoistOpts) {
  const result = getHoistedDependencies(opts)
  if (!result) return {}
  const { hoistedDependencies, hoistedAliasesWithBins } = result

  await symlinkHoistedDependencies(hoistedDependencies, {
    lockfile: opts.lockfile,
    privateHoistedModulesDir: opts.privateHoistedModulesDir,
    publicHoistedModulesDir: opts.publicHoistedModulesDir,
    virtualStoreDir: opts.virtualStoreDir,
    hoistWorkspaceProjects: opts.hoistWorkspaceProjects,
  })

  // Here we only link the bins of the privately hoisted modules.
  // The bins of the publicly hoisted modules will be linked together with
  // the bins of the project's direct dependencies.
  // This is possible because the publicly hoisted modules
  // are in the same directory as the regular dependencies.
  await linkAllBins(opts.privateHoistedModulesDir, {
    extraNodePaths: opts.extraNodePath,
    hoistedAliasesWithBins,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
  })

  return hoistedDependencies
}

export interface GetHoistedDependenciesOpts {
  lockfile: Lockfile
  importerIds?: string[]
  privateHoistPattern: string[]
  privateHoistedModulesDir: string
  publicHoistPattern: string[]
  publicHoistedModulesDir: string
  hoistWorkspaceProjects?: boolean
  allProjects: Record<string, Project>
}

export function getHoistedDependencies (opts: GetHoistedDependenciesOpts) {
  if (opts.lockfile.packages == null) return null

  const { directDeps, step } = lockfileWalker(
    opts.lockfile,
    opts.importerIds ?? Object.keys(opts.lockfile.importers)
  )
  const deps = [
    {
      children: directDeps
        .reduce((acc, { alias, depPath }) => {
          if (!acc[alias]) {
            acc[alias] = depPath
          }
          return acc
        }, {} as Record<string, string>),
      depPath: '',
      depth: -1,
    },
    ...getDependencies(0, step),
  ]

  const getAliasHoistType = createGetAliasHoistType(opts.publicHoistPattern, opts.privateHoistPattern)

  const hoistedProjects: Dependency[] = []

  if (opts.hoistWorkspaceProjects && opts.allProjects) {
    const { allProjects } = opts
    const hoistedWorkspaceProjects = Object.entries(allProjects)
    const workspace = allProjects['.']
    hoistedWorkspaceProjects
    // Ignore the project "." because it is the root project
      .filter(([, project]) => project.id !== '.')
      .forEach(([, project]) => {
        hoistedProjects.push({
          children: {
            [project.id]: path.normalize(path.relative(workspace.rootDir, project.rootDir)),
          },
          depPath: '',
          depth: -1,
        })
      })
  }

  return hoistGraph(deps.concat(hoistedProjects), opts.lockfile.importers['.']?.specifiers ?? {}, {
    getAliasHoistType,
    lockfile: opts.lockfile,
  })
}

type GetAliasHoistType = (alias: string) => 'private' | 'public' | false

function createGetAliasHoistType (
  publicHoistPattern: string[],
  privateHoistPattern: string[]
): GetAliasHoistType {
  const publicMatcher = createMatcher(publicHoistPattern)
  const privateMatcher = createMatcher(privateHoistPattern)
  return (alias: string) => {
    if (publicMatcher(alias)) return 'public'
    if (privateMatcher(alias)) return 'private'
    return false
  }
}

interface LinkAllBinsOptions {
  extraNodePaths?: string[]
  hoistedAliasesWithBins: string[]
  preferSymlinkedExecutables?: boolean
}

async function linkAllBins (modulesDir: string, opts: LinkAllBinsOptions) {
  const bin = path.join(modulesDir, '.bin')
  const warn: WarnFunction = (message, code) => {
    if (code === 'BINARIES_CONFLICT') return
    logger.info({ message, prefix: path.join(modulesDir, '../..') })
  }
  try {
    await linkBinsOfPkgsByAliases(opts.hoistedAliasesWithBins, bin, {
      allowExoticManifests: true,
      extraNodePaths: opts.extraNodePaths,
      modulesDir,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn,
    })
  } catch (err: any) { // eslint-disable-line
    // Some packages generate their commands with lifecycle hooks.
    // At this stage, such commands are not generated yet.
    // For now, we don't hoist such generated commands.
    // Related issue: https://github.com/pnpm/pnpm/issues/2071
  }
}

function getDependencies (
  depth: number,
  step: LockfileWalkerStep
): Dependency[] {
  const deps: Dependency[] = []
  const nextSteps: LockfileWalkerStep[] = []
  for (const { pkgSnapshot, depPath, next } of step.dependencies) {
    const allDeps: Record<string, string> = {
      ...pkgSnapshot.dependencies,
      ...pkgSnapshot.optionalDependencies,
    }
    deps.push({
      children: mapObjIndexed(dp.refToRelative, allDeps) as Record<string, string>,
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

  return [
    ...deps,
    ...nextSteps.flatMap(getDependencies.bind(null, depth + 1)),
  ]
}

export interface Dependency {
  children: Record<string, string>
  depPath: string
  depth: number
}

interface HoistGraphResult {
  hoistedDependencies: HoistedDependencies
  hoistedAliasesWithBins: string[]
}

function hoistGraph (
  depNodes: Dependency[],
  currentSpecifiers: Record<string, string>,
  opts: {
    getAliasHoistType: GetAliasHoistType
    lockfile: Lockfile
  }
): HoistGraphResult {
  const hoistedAliases = new Set(Object.keys(currentSpecifiers))
  const hoistedDependencies: HoistedDependencies = {}
  const hoistedAliasesWithBins = new Set<string>()

  depNodes
    // sort by depth and then alphabetically
    .sort((a, b) => {
      const depthDiff = a.depth - b.depth
      return depthDiff === 0 ? lexCompare(a.depPath, b.depPath) : depthDiff
    })
    // build the alias map and the id map
    .forEach((depNode) => {
      for (const [childAlias, childPath] of Object.entries(depNode.children)) {
        const hoist = opts.getAliasHoistType(childAlias)
        if (!hoist) continue
        const childAliasNormalized = childAlias.toLowerCase()
        // if this alias has already been taken, skip it
        if (hoistedAliases.has(childAliasNormalized)) {
          continue
        }
        if (opts.lockfile.packages?.[childPath]?.hasBin) {
          hoistedAliasesWithBins.add(childAlias)
        }
        hoistedAliases.add(childAliasNormalized)
        if (!hoistedDependencies[childPath]) {
          hoistedDependencies[childPath] = {}
        }
        hoistedDependencies[childPath][childAlias] = hoist
      }
    })

  return { hoistedDependencies, hoistedAliasesWithBins: Array.from(hoistedAliasesWithBins) }
}

async function symlinkHoistedDependencies (
  hoistedDependencies: HoistedDependencies,
  opts: {
    lockfile: Lockfile
    privateHoistedModulesDir: string
    publicHoistedModulesDir: string
    virtualStoreDir: string
    hoistWorkspaceProjects?: boolean
    allProjects?: Record<string, Project>
  }
) {
  const symlink = symlinkHoistedDependency.bind(null, opts)
  await Promise.all(
    Object.entries(hoistedDependencies)
      .map(async ([depPath, pkgAliases]) => {
        const pkgSnapshot = opts.lockfile.packages![depPath]
        // Only get the projects if hoistWorkspaceProjects is true
        const isProject = opts.hoistWorkspaceProjects ? opts.lockfile.importers[depPath] : false
        if (!pkgSnapshot && !isProject) {
          // This dependency is probably a skipped optional dependency.
          hoistLogger.debug({ hoistFailedFor: depPath })
          return
        }
        const pkgName = !isProject
          ? nameVerFromPkgSnapshot(depPath, pkgSnapshot).name
          : opts.allProjects![depPath].id

        const modules = !isProject
          ? path.join(opts.virtualStoreDir, dp.depPathToFilename(depPath), 'node_modules')
          : path.join(path.dirname(opts.publicHoistedModulesDir), depPath)

        const depLocation = !isProject
          ? path.join(modules, pkgName)
          : modules
        await Promise.all(Object.entries(pkgAliases).map(async ([pkgAlias, hoistType]) => {
          const targetDir = hoistType === 'public'
            ? opts.publicHoistedModulesDir
            : opts.privateHoistedModulesDir
          const dest = path.join(targetDir, pkgAlias)
          return symlink(depLocation, dest)
        }))
      })
  )
}

async function symlinkHoistedDependency (
  opts: { virtualStoreDir: string },
  depLocation: string,
  dest: string
) {
  try {
    await symlinkDir(depLocation, dest, { overwrite: false })
    linkLogger.debug({ target: dest, link: depLocation })
    return
  } catch (err: any) { // eslint-disable-line
    if (err.code !== 'EEXIST' && err.code !== 'EISDIR') throw err
  }
  let existingSymlink!: string
  try {
    existingSymlink = await resolveLinkTarget(dest)
  } catch (err) {
    hoistLogger.debug({
      skipped: dest,
      reason: 'a directory is present at the target location',
    })
    return
  }
  if (!isSubdir(opts.virtualStoreDir, existingSymlink)) {
    hoistLogger.debug({
      skipped: dest,
      existingSymlink,
      reason: 'an external symlink is present at the target location',
    })
    return
  }
  await fs.promises.unlink(dest)
  await symlinkDir(depLocation, dest)
  linkLogger.debug({ target: dest, link: depLocation })
}