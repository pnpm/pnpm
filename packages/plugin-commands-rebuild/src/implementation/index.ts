import {
  LAYOUT_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import getContext from '@pnpm/get-context'
import {
  runLifecycleHooksConcurrently,
  runPostinstallHooks,
} from '@pnpm/lifecycle'
import linkBins from '@pnpm/link-bins'
import {
  Lockfile,
  nameVerFromPkgSnapshot,
  packageIsIndependent,
  PackageSnapshots,
} from '@pnpm/lockfile-utils'
import lockfileWalker, { LockfileWalkerStep } from '@pnpm/lockfile-walker'
import logger, { streamParser } from '@pnpm/logger'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { ProjectManifest } from '@pnpm/types'
import * as dp from 'dependency-path'
import runGroups from 'run-groups'
import extendOptions, {
  RebuildOptions,
  StrictRebuildOptions,
} from './extendRebuildOptions'
import path = require('path')
import npa = require('@zkochan/npm-package-arg')
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
import R = require('ramda')
import semver = require('semver')

export { RebuildOptions }

function findPackages (
  packages: PackageSnapshots,
  searched: PackageSelector[],
  opts: {
    prefix: string
  }
): string[] {
  return Object.keys(packages)
    .filter((relativeDepPath) => {
      const pkgLockfile = packages[relativeDepPath]
      const pkgInfo = nameVerFromPkgSnapshot(relativeDepPath, pkgLockfile)
      if (!pkgInfo.name) {
        logger.warn({
          message: `Skipping ${relativeDepPath} because cannot get the package name from ${WANTED_LOCKFILE}.
            Try to run run \`pnpm update --depth 100\` to create a new ${WANTED_LOCKFILE} with all the necessary info.`,
          prefix: opts.prefix,
        })
        return false
      }
      return matches(searched, pkgInfo)
    })
}

// TODO: move this logic to separate package as this is also used in dependencies-hierarchy
function matches (
  searched: PackageSelector[],
  manifest: {name: string, version?: string}
) {
  return searched.some((searchedPkg) => {
    if (typeof searchedPkg === 'string') {
      return manifest.name === searchedPkg
    }
    return searchedPkg.name === manifest.name && !!manifest.version &&
      semver.satisfies(manifest.version, searchedPkg.range)
  })
}

type PackageSelector = string | {
  name: string
  range: string
}

export async function rebuildPkgs (
  projects: Array<{ manifest: ProjectManifest, rootDir: string }>,
  pkgSpecs: string[],
  maybeOpts: RebuildOptions
) {
  const reporter = maybeOpts?.reporter
  if (reporter && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContext(projects, opts)

  if (!ctx.currentLockfile || !ctx.currentLockfile.packages) return
  const packages = ctx.currentLockfile.packages

  const searched: PackageSelector[] = pkgSpecs.map((arg) => {
    const { fetchSpec, name, raw, type } = npa(arg)
    if (raw === name) {
      return name
    }
    if (type !== 'version' && type !== 'range') {
      throw new Error(`Invalid argument - ${arg}. Rebuild can only select by version or range`)
    }
    return {
      name,
      range: fetchSpec,
    }
  })

  let pkgs = [] as string[]
  for (const { rootDir } of projects) {
    pkgs = [
      ...pkgs,
      ...findPackages(packages, searched, { prefix: rootDir }),
    ]
  }

  await _rebuild(
    {
      pkgsToRebuild: new Set(pkgs),
      ...ctx,
    },
    opts
  )
}

export async function rebuild (
  projects: Array<{ buildIndex: number, manifest: ProjectManifest, rootDir: string }>,
  maybeOpts: RebuildOptions
) {
  const reporter = maybeOpts?.reporter
  if (reporter && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContext(projects, opts)

  let idsToRebuild: string[] = []

  if (opts.pending) {
    idsToRebuild = ctx.pendingBuilds
  } else if (ctx.currentLockfile?.packages) {
    idsToRebuild = Object.keys(ctx.currentLockfile.packages)
  }

  const pkgsThatWereRebuilt = await _rebuild(
    {
      pkgsToRebuild: new Set(idsToRebuild),
      ...ctx,
    },
    opts
  )

  ctx.pendingBuilds = ctx.pendingBuilds.filter((depPath) => !pkgsThatWereRebuilt.has(depPath))

  const scriptsOpts = {
    extraBinPaths: ctx.extraBinPaths,
    rawConfig: opts.rawConfig,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    unsafePerm: opts.unsafePerm || false,
  }
  await runLifecycleHooksConcurrently(
    ['preinstall', 'install', 'postinstall', 'prepublish', 'prepare'],
    ctx.projects,
    opts.childConcurrency || 5,
    scriptsOpts
  )
  for (const { id, manifest } of ctx.projects) {
    if (manifest?.scripts && (!opts.pending || ctx.pendingBuilds.includes(id))) {
      ctx.pendingBuilds.splice(ctx.pendingBuilds.indexOf(id), 1)
    }
  }

  await writeModulesYaml(ctx.rootModulesDir, {
    ...ctx.modulesFile,
    hoistedDependencies: ctx.hoistedDependencies,
    hoistPattern: ctx.hoistPattern,
    included: ctx.include,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    publicHoistPattern: ctx.publicHoistPattern,
    registries: ctx.registries,
    skipped: Array.from(ctx.skipped),
    storeDir: ctx.storeDir,
    virtualStoreDir: ctx.virtualStoreDir,
  })
}

function getSubgraphToBuild (
  step: LockfileWalkerStep,
  nodesToBuildAndTransitive: Set<string>,
  opts: {
    pkgsToRebuild: Set<string>
  }
) {
  let currentShouldBeBuilt = false
  for (const { depPath, next } of step.dependencies) {
    if (nodesToBuildAndTransitive.has(depPath)) {
      currentShouldBeBuilt = true
    }

    const childShouldBeBuilt = getSubgraphToBuild(next(), nodesToBuildAndTransitive, opts) ||
      opts.pkgsToRebuild.has(depPath)
    if (childShouldBeBuilt) {
      nodesToBuildAndTransitive.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  for (const depPath of step.missing) {
    // It might make sense to fail if the depPath is not in the skipped list from .modules.yaml
    // However, the skipped list currently contains package IDs, not dep paths.
    logger.debug({ message: `No entry for "${depPath}" in ${WANTED_LOCKFILE}` })
  }
  return currentShouldBeBuilt
}

const limitLinking = pLimit(16)

async function _rebuild (
  ctx: {
    pkgsToRebuild: Set<string>
    virtualStoreDir: string
    rootModulesDir: string
    currentLockfile: Lockfile
    projects: Array<{ id: string, rootDir: string }>
    extraBinPaths: string[]
  },
  opts: StrictRebuildOptions
) {
  const pkgsThatWereRebuilt = new Set()
  const graph = new Map()
  const pkgSnapshots: PackageSnapshots = ctx.currentLockfile.packages ?? {}

  const nodesToBuildAndTransitive = new Set<string>()
  getSubgraphToBuild(
    lockfileWalker(
      ctx.currentLockfile,
      ctx.projects.map(({ id }) => id),
      {
        include: {
          dependencies: opts.production,
          devDependencies: opts.development,
          optionalDependencies: opts.optional,
        },
      }
    ).step,
    nodesToBuildAndTransitive,
    { pkgsToRebuild: ctx.pkgsToRebuild }
  )
  const nodesToBuildAndTransitiveArray = Array.from(nodesToBuildAndTransitive)

  for (const depPath of nodesToBuildAndTransitiveArray) {
    const pkgSnapshot = pkgSnapshots[depPath]
    graph.set(depPath, R.toPairs({ ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })
      .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
      .filter((childRelDepPath) => childRelDepPath && nodesToBuildAndTransitive.has(childRelDepPath)))
  }
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [nodesToBuildAndTransitiveArray],
  })
  const chunks = graphSequencerResult.chunks as string[][]
  const warn = (message: string) => logger.info({ message, prefix: opts.dir })
  const groups = chunks.map((chunk) => chunk.filter((depPath) => ctx.pkgsToRebuild.has(depPath)).map((depPath) =>
    async () => {
      const pkgSnapshot = pkgSnapshots[depPath]
      const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      const pkgRoot = path.join(ctx.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDir), 'node_modules', pkgInfo.name)
      try {
        const modules = path.join(ctx.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDir), 'node_modules')
        const binPath = path.join(pkgRoot, 'node_modules', '.bin')
        await linkBins(modules, binPath, { warn })
        await runPostinstallHooks({
          depPath,
          extraBinPaths: ctx.extraBinPaths,
          optional: pkgSnapshot.optional === true,
          pkgRoot,
          prepare: pkgSnapshot.prepare,
          rawConfig: opts.rawConfig,
          rootModulesDir: ctx.rootModulesDir,
          shellEmulator: opts.shellEmulator,
          unsafePerm: opts.unsafePerm || false,
        })
        pkgsThatWereRebuilt.add(depPath)
      } catch (err) {
        if (pkgSnapshot.optional) {
          // TODO: add parents field to the log
          skippedOptionalDependencyLogger.debug({
            details: err.toString(),
            package: {
              id: pkgSnapshot.id ?? depPath,
              name: pkgInfo.name,
              version: pkgInfo.version,
            },
            prefix: opts.dir,
            reason: 'build_failure',
          })
          return
        }
        throw err
      }
    }
  ))

  await runGroups(opts.childConcurrency || 5, groups)

  // It may be optimized because some bins were already linked before running lifecycle scripts
  await Promise.all(
    Object
      .keys(pkgSnapshots)
      .filter((depPath) => !packageIsIndependent(pkgSnapshots[depPath]))
      .map((depPath) => limitLinking(() => {
        const pkgSnapshot = pkgSnapshots[depPath]
        const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        const modules = path.join(ctx.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDir), 'node_modules')
        const binPath = path.join(modules, pkgInfo.name, 'node_modules', '.bin')
        return linkBins(modules, binPath, { warn })
      }))
  )
  await Promise.all(ctx.projects.map(({ rootDir }) => limitLinking(() => {
    const modules = path.join(rootDir, 'node_modules')
    const binPath = path.join(modules, '.bin')
    return linkBins(modules, binPath, {
      allowExoticManifests: true,
      warn,
    })
  })))

  return pkgsThatWereRebuilt
}
