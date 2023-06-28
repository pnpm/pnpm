import path from 'path'
import {
  LAYOUT_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import { getContext, type PnpmContext } from '@pnpm/get-context'
import {
  runLifecycleHooksConcurrently,
  runPostinstallHooks,
} from '@pnpm/lifecycle'
import { linkBins } from '@pnpm/link-bins'
import {
  type Lockfile,
  nameVerFromPkgSnapshot,
  packageIsIndependent,
  type PackageSnapshots,
} from '@pnpm/lockfile-utils'
import { lockfileWalker, type LockfileWalkerStep } from '@pnpm/lockfile-walker'
import { logger, streamParser } from '@pnpm/logger'
import { writeModulesManifest } from '@pnpm/modules-yaml'
import { createOrConnectStoreController } from '@pnpm/store-connection-manager'
import { type ProjectManifest } from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import { hardLinkDir } from '@pnpm/fs.hard-link-dir'
import runGroups from 'run-groups'
import graphSequencer from '@pnpm/graph-sequencer'
import npa from '@pnpm/npm-package-arg'
import pLimit from 'p-limit'
import semver from 'semver'
import {
  extendRebuildOptions,
  type RebuildOptions,
  type StrictRebuildOptions,
} from './extendRebuildOptions'

export type { RebuildOptions }

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
  manifest: { name: string, version?: string }
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

export async function rebuildSelectedPkgs (
  projects: Array<{ buildIndex: number, manifest: ProjectManifest, rootDir: string }>,
  pkgSpecs: string[],
  maybeOpts: RebuildOptions
) {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendRebuildOptions(maybeOpts)
  const ctx = await getContext({ ...opts, allProjects: projects })

  if (!ctx.currentLockfile || (ctx.currentLockfile.packages == null)) return
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

export async function rebuildProjects (
  projects: Array<{ buildIndex: number, manifest: ProjectManifest, rootDir: string }>,
  maybeOpts: RebuildOptions
) {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendRebuildOptions(maybeOpts)
  const ctx = await getContext({ ...opts, allProjects: projects })

  let idsToRebuild: string[] = []

  if (opts.pending) {
    idsToRebuild = ctx.pendingBuilds
  } else if ((ctx.currentLockfile?.packages) != null) {
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

  const store = await createOrConnectStoreController(opts)
  const scriptsOpts = {
    extraBinPaths: ctx.extraBinPaths,
    extraNodePaths: ctx.extraNodePaths,
    extraEnv: opts.extraEnv,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    rawConfig: opts.rawConfig,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    storeController: store.ctrl,
    unsafePerm: opts.unsafePerm || false,
  }
  await runLifecycleHooksConcurrently(
    ['preinstall', 'install', 'postinstall', 'prepublish', 'prepare'],
    Object.values(ctx.projects),
    opts.childConcurrency || 5,
    scriptsOpts
  )
  for (const { id, manifest } of Object.values(ctx.projects)) {
    if (((manifest?.scripts) != null) && (!opts.pending || ctx.pendingBuilds.includes(id))) {
      ctx.pendingBuilds.splice(ctx.pendingBuilds.indexOf(id), 1)
    }
  }

  await writeModulesManifest(ctx.rootModulesDir, {
    prunedAt: new Date().toUTCString(),
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
    skipped: Set<string>
    virtualStoreDir: string
    rootModulesDir: string
    currentLockfile: Lockfile
    projects: Record<string, { id: string, rootDir: string }>
    extraBinPaths: string[]
    extraNodePaths: string[]
  } & Pick<PnpmContext, 'modulesFile'>,
  opts: StrictRebuildOptions
) {
  const pkgsThatWereRebuilt = new Set()
  const graph = new Map()
  const pkgSnapshots: PackageSnapshots = ctx.currentLockfile.packages ?? {}

  const nodesToBuildAndTransitive = new Set<string>()
  getSubgraphToBuild(
    lockfileWalker(
      ctx.currentLockfile,
      Object.values(ctx.projects).map(({ id }) => id),
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
    graph.set(depPath, Object.entries({ ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })
      .map(([pkgName, reference]) => dp.refToRelative(reference, pkgName))
      .filter((childRelDepPath) => childRelDepPath && nodesToBuildAndTransitive.has(childRelDepPath)))
  }
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [nodesToBuildAndTransitiveArray],
  })
  const chunks = graphSequencerResult.chunks as string[][]
  const warn = (message: string) => {
    logger.info({ message, prefix: opts.dir })
  }
  const groups = chunks.map((chunk) => chunk.filter((depPath) => ctx.pkgsToRebuild.has(depPath) && !ctx.skipped.has(depPath)).map((depPath) =>
    async () => {
      const pkgSnapshot = pkgSnapshots[depPath]
      const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      const pkgRoots = opts.nodeLinker === 'hoisted'
        ? (ctx.modulesFile?.hoistedLocations?.[depPath] ?? []).map((hoistedLocation) => path.join(opts.lockfileDir, hoistedLocation))
        : [path.join(ctx.virtualStoreDir, dp.depPathToFilename(depPath), 'node_modules', pkgInfo.name)]
      if (pkgRoots.length === 0) {
        if (pkgSnapshot.optional) return
        throw new PnpmError('MISSING_HOISTED_LOCATIONS', `${depPath} is not found in hoistedLocations inside node_modules/.modules.yaml`, {
          hint: 'If you installed your node_modules with pnpm older than v7.19.0, you may need to remove it and run "pnpm install"',
        })
      }
      const pkgRoot = pkgRoots[0]
      try {
        const extraBinPaths = ctx.extraBinPaths
        if (opts.nodeLinker !== 'hoisted') {
          const modules = path.join(ctx.virtualStoreDir, dp.depPathToFilename(depPath), 'node_modules')
          const binPath = path.join(pkgRoot, 'node_modules', '.bin')
          await linkBins(modules, binPath, { extraNodePaths: ctx.extraNodePaths, warn })
        } else {
          extraBinPaths.push(...binDirsInAllParentDirs(pkgRoot, opts.lockfileDir))
        }
        await runPostinstallHooks({
          depPath,
          extraBinPaths,
          extraEnv: opts.extraEnv,
          optional: pkgSnapshot.optional === true,
          pkgRoot,
          rawConfig: opts.rawConfig,
          rootModulesDir: ctx.rootModulesDir,
          scriptsPrependNodePath: opts.scriptsPrependNodePath,
          shellEmulator: opts.shellEmulator,
          unsafePerm: opts.unsafePerm || false,
        })
        pkgsThatWereRebuilt.add(depPath)
      } catch (err: any) { // eslint-disable-line
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
      if (pkgRoots.length > 1) {
        await hardLinkDir(pkgRoot, pkgRoots.slice(1))
      }
    }
  ))

  await runGroups(opts.childConcurrency || 5, groups)

  // It may be optimized because some bins were already linked before running lifecycle scripts
  await Promise.all(
    Object
      .keys(pkgSnapshots)
      .filter((depPath) => !packageIsIndependent(pkgSnapshots[depPath]))
      .map(async (depPath) => limitLinking(async () => {
        const pkgSnapshot = pkgSnapshots[depPath]
        const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        const modules = path.join(ctx.virtualStoreDir, dp.depPathToFilename(depPath), 'node_modules')
        const binPath = path.join(modules, pkgInfo.name, 'node_modules', '.bin')
        return linkBins(modules, binPath, { warn })
      }))
  )
  await Promise.all(Object.values(ctx.projects).map(async ({ rootDir }) => limitLinking(async () => {
    const modules = path.join(rootDir, 'node_modules')
    const binPath = path.join(modules, '.bin')
    return linkBins(modules, binPath, {
      allowExoticManifests: true,
      warn,
    })
  })))

  return pkgsThatWereRebuilt
}

function binDirsInAllParentDirs (pkgRoot: string, lockfileDir: string): string[] {
  const binDirs: string[] = []
  let dir = pkgRoot
  do {
    if (!path.dirname(dir).startsWith('@')) {
      binDirs.push(path.join(dir, 'node_modules/.bin'))
    }
    dir = path.dirname(dir)
  } while (path.relative(dir, lockfileDir) !== '')
  binDirs.push(path.join(lockfileDir, 'node_modules/.bin'))
  return binDirs
}
