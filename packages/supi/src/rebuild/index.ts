import {
  ENGINE_NAME,
  LAYOUT_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
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
import logger, { streamParser } from '@pnpm/logger'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { ImporterManifest } from '@pnpm/types'
import npa = require('@zkochan/npm-package-arg')
import * as dp from 'dependency-path'
import graphSequencer = require('graph-sequencer')
import pLimit from 'p-limit'
import path = require('path')
import R = require('ramda')
import runGroups from 'run-groups'
import semver = require('semver')
import getContext from '../getContext'
import extendOptions, {
  RebuildOptions,
  StrictRebuildOptions,
} from './extendRebuildOptions'

function findPackages (
  packages: PackageSnapshots,
  searched: PackageSelector[],
  opts: {
    prefix: string,
  },
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
  manifest: {name: string, version?: string},
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
  name: string,
  range: string,
}

export async function rebuildPkgs (
  importers: Array<{ manifest: ImporterManifest, prefix: string }>,
  pkgSpecs: string[],
  maybeOpts: RebuildOptions,
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContext(importers, opts)

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
  for (const { prefix } of importers) {
    pkgs = [
      ...pkgs,
      ...findPackages(packages, searched, { prefix }),
    ]
  }

  await _rebuild(
    {
      pkgsToRebuild: new Set(pkgs),
      ...ctx,
    },
    opts,
  )
}

export async function rebuild (
  importers: Array<{ buildIndex: number, manifest: ImporterManifest, prefix: string }>,
  maybeOpts: RebuildOptions,
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContext(importers, opts)

  let idsToRebuild: string[] = []

  if (opts.pending) {
    idsToRebuild = ctx.pendingBuilds
  } else if (ctx.currentLockfile && ctx.currentLockfile.packages) {
    idsToRebuild = Object.keys(ctx.currentLockfile.packages)
  }

  const pkgsThatWereRebuilt = await _rebuild(
    {
      pkgsToRebuild: new Set(idsToRebuild),
      ...ctx,
    },
    opts,
  )

  ctx.pendingBuilds = ctx.pendingBuilds.filter((relDepPath) => !pkgsThatWereRebuilt.has(relDepPath))

  const scriptsOpts = {
    extraBinPaths: ctx.extraBinPaths,
    rawConfig: opts.rawConfig,
    unsafePerm: opts.unsafePerm || false,
  }
  await runLifecycleHooksConcurrently(
    ['preinstall', 'install', 'postinstall', 'prepublish', 'prepare'],
    ctx.importers,
    opts.childConcurrency || 5,
    scriptsOpts,
  )
  for (const { id, manifest } of ctx.importers) {
    if (manifest && manifest.scripts && (!opts.pending || ctx.pendingBuilds.includes(id))) {
      ctx.pendingBuilds.splice(ctx.pendingBuilds.indexOf(id), 1)
    }
  }

  await writeModulesYaml(ctx.rootModulesDir, {
    ...ctx.modulesFile,
    hoistedAliases: ctx.hoistedAliases,
    hoistPattern: ctx.hoistPattern,
    included: ctx.include,
    independentLeaves: ctx.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    registries: ctx.registries,
    shamefullyHoist: ctx.shamefullyHoist,
    skipped: Array.from(ctx.skipped),
    store: ctx.storePath,
    virtualStoreDir: ctx.virtualStoreDir,
  })
}

function getSubgraphToBuild (
  pkgSnapshots: PackageSnapshots,
  entryNodes: string[],
  nodesToBuildAndTransitive: Set<string>,
  walked: Set<string>,
  opts: {
    optional: boolean,
    pkgsToRebuild: Set<string>,
  },
) {
  let currentShouldBeBuilt = false
  for (const depPath of entryNodes) {
    if (nodesToBuildAndTransitive.has(depPath)) {
      currentShouldBeBuilt = true
    }
    if (walked.has(depPath)) continue
    walked.add(depPath)
    const pkgSnapshot = pkgSnapshots[depPath]
    if (!pkgSnapshot) {
      if (depPath.startsWith('link:')) continue

      // It might make sense to fail if the depPath is not in the skipped list from .modules.yaml
      // However, the skipped list currently contains package IDs, not dep paths.
      logger.debug({ message: `No entry for "${depPath}" in ${WANTED_LOCKFILE}` })
      continue
    }
    const nextEntryNodes = R.toPairs({
      ...pkgSnapshot.dependencies,
      ...(opts.optional && pkgSnapshot.optionalDependencies || {}),
    })
    .map(([ pkgName, reference ]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null) as string[]

    const childShouldBeBuilt = getSubgraphToBuild(pkgSnapshots, nextEntryNodes, nodesToBuildAndTransitive, walked, opts)
      || opts.pkgsToRebuild.has(depPath)
    if (childShouldBeBuilt) {
      nodesToBuildAndTransitive.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  return currentShouldBeBuilt
}

const limitLinking = pLimit(16)

async function _rebuild (
  ctx: {
    pkgsToRebuild: Set<string>,
    virtualStoreDir: string,
    rootModulesDir: string,
    currentLockfile: Lockfile,
    importers: Array<{ id: string, prefix: string }>,
    independentLeaves: boolean,
    extraBinPaths: string[],
  },
  opts: StrictRebuildOptions,
) {
  const pkgsThatWereRebuilt = new Set()
  const graph = new Map()
  const pkgSnapshots: PackageSnapshots = ctx.currentLockfile.packages || {}

  const entryNodes = [] as string[]

  ctx.importers.forEach((importer) => {
    const lockfileImporter = ctx.currentLockfile.importers[importer.id]
    R.toPairs({
      ...(opts.development && lockfileImporter.devDependencies || {}),
      ...(opts.production && lockfileImporter.dependencies || {}),
      ...(opts.optional && lockfileImporter.optionalDependencies || {}),
    })
    .map(([ pkgName, reference ]) => dp.refToRelative(reference, pkgName))
    .filter((nodeId) => nodeId !== null)
    .forEach((relDepPath) => {
      entryNodes.push(relDepPath as string)
    })
  })

  const nodesToBuildAndTransitive = new Set<string>()
  getSubgraphToBuild(pkgSnapshots, entryNodes, nodesToBuildAndTransitive, new Set(), {
    optional: opts.optional === true,
    pkgsToRebuild: ctx.pkgsToRebuild,
  })
  const nodesToBuildAndTransitiveArray = Array.from(nodesToBuildAndTransitive)

  for (const relDepPath of nodesToBuildAndTransitiveArray) {
    const pkgSnapshot = pkgSnapshots[relDepPath]
    graph.set(relDepPath, R.toPairs({ ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })
      .map(([ pkgName, reference ]) => dp.refToRelative(reference, pkgName))
      .filter((childRelDepPath) => childRelDepPath && nodesToBuildAndTransitive.has(childRelDepPath)))
  }
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [nodesToBuildAndTransitiveArray],
  })
  const chunks = graphSequencerResult.chunks as string[][]
  const warn = (message: string) => logger.warn({ message, prefix: opts.prefix })
  const groups = chunks.map((chunk) => chunk.filter((relDepPath) => ctx.pkgsToRebuild.has(relDepPath)).map((relDepPath) =>
    async () => {
      const pkgSnapshot = pkgSnapshots[relDepPath]
      const depPath = dp.resolve(opts.registries, relDepPath)
      const pkgInfo = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
      const independent = ctx.independentLeaves && packageIsIndependent(pkgSnapshot)
      const pkgRoot = !independent
        ? path.join(ctx.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDirectory), 'node_modules', pkgInfo.name)
        : await (
          async () => {
            const { directory } = await opts.storeController.getPackageLocation(pkgSnapshot.id || depPath, pkgInfo.name, {
              lockfileDirectory: opts.lockfileDirectory,
              targetEngine: opts.sideEffectsCacheRead && !opts.force && ENGINE_NAME || undefined,
            })
            return directory
          }
        )()
      try {
        if (!independent) {
          const modules = path.join(ctx.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDirectory), 'node_modules')
          const binPath = path.join(pkgRoot, 'node_modules', '.bin')
          await linkBins(modules, binPath, { warn })
        }
        await runPostinstallHooks({
          depPath,
          extraBinPaths: ctx.extraBinPaths,
          optional: pkgSnapshot.optional === true,
          pkgRoot,
          prepare: pkgSnapshot.prepare,
          rawConfig: opts.rawConfig,
          rootNodeModulesDir: ctx.rootModulesDir,
          unsafePerm: opts.unsafePerm || false,
        })
        pkgsThatWereRebuilt.add(relDepPath)
      } catch (err) {
        if (pkgSnapshot.optional) {
          // TODO: add parents field to the log
          skippedOptionalDependencyLogger.debug({
            details: err.toString(),
            package: {
              id: pkgSnapshot.id || depPath,
              name: pkgInfo.name,
              version: pkgInfo.version,
            },
            prefix: opts.prefix,
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
      .filter((relDepPath) => !packageIsIndependent(pkgSnapshots[relDepPath]))
      .map((relDepPath) => limitLinking(() => {
        const depPath = dp.resolve(opts.registries, relDepPath)
        const pkgSnapshot = pkgSnapshots[relDepPath]
        const pkgInfo = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
        const modules = path.join(ctx.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDirectory), 'node_modules')
        const binPath = path.join(modules, pkgInfo.name, 'node_modules', '.bin')
        return linkBins(modules, binPath, { warn })
      })),
  )
  await Promise.all(ctx.importers.map(({ prefix }) => limitLinking(() => {
    const modules = path.join(prefix, 'node_modules')
    const binPath = path.join(modules, '.bin')
    return linkBins(modules, binPath, {
      allowExoticManifests: true,
      warn,
    })
  })))

  return pkgsThatWereRebuilt
}
