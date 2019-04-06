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
import npa = require('@zkochan/npm-package-arg')
import * as dp from 'dependency-path'
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
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
  return R.keys(packages)
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
  pkg: {name: string, version?: string},
) {
  return searched.some((searchedPkg) => {
    if (typeof searchedPkg === 'string') {
      return pkg.name === searchedPkg
    }
    return searchedPkg.name === pkg.name && !!pkg.version &&
      semver.satisfies(pkg.version, searchedPkg.range)
  })
}

type PackageSelector = string | {
  name: string,
  range: string,
}

export async function rebuildPkgs (
  importers: Array<{ prefix: string }>,
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
    const parsed = npa(arg)
    if (parsed.raw === parsed.name) {
      return parsed.name
    }
    if (parsed.type !== 'version' && parsed.type !== 'range') {
      throw new Error(`Invalid argument - ${arg}. Rebuild can only select by version or range`)
    }
    return {
      name: parsed.name,
      range: parsed.fetchSpec,
    }
  })

  let pkgs = [] as string[]
  for (const importer of importers) {
    pkgs = [
      ...pkgs,
      ...findPackages(packages, searched, { prefix: importer.prefix }),
    ]
  }

  await _rebuild(
    new Set(pkgs),
    ctx.virtualStoreDir,
    ctx.currentLockfile,
    ctx.importers,
    opts,
  )
}

export async function rebuild (
  importers: Array<{ buildIndex: number, prefix: string }>,
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
    idsToRebuild = R.keys(ctx.currentLockfile.packages)
  }

  const pkgsThatWereRebuilt = await _rebuild(
    new Set(idsToRebuild),
    ctx.virtualStoreDir,
    ctx.currentLockfile,
    ctx.importers,
    opts,
  )

  ctx.pendingBuilds = ctx.pendingBuilds.filter((relDepPath) => !pkgsThatWereRebuilt.has(relDepPath))

  const scriptsOpts = {
    rawNpmConfig: opts.rawNpmConfig,
    unsafePerm: opts.unsafePerm || false,
  }
  await runLifecycleHooksConcurrently(
    ['preinstall', 'install', 'postinstall', 'prepublish', 'prepare'],
    ctx.importers,
    opts.childConcurrency || 5,
    scriptsOpts,
  )
  for (const importer of ctx.importers) {
    if (importer.pkg && importer.pkg.scripts && (!opts.pending || ctx.pendingBuilds.indexOf(importer.id) !== -1)) {
      ctx.pendingBuilds.splice(ctx.pendingBuilds.indexOf(importer.id), 1)
    }
  }

  await writeModulesYaml(ctx.virtualStoreDir, {
    ...ctx.modulesFile,
    importers: {
      ...ctx.modulesFile && ctx.modulesFile.importers,
      ...ctx.importers.reduce((acc, importer) => {
        acc[importer.id] = {
          hoistedAliases: importer.hoistedAliases,
          shamefullyFlatten: importer.shamefullyFlatten,
        }
        return acc
      }, {}),
    },
    included: ctx.include,
    independentLeaves: opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: ctx.pendingBuilds,
    registries: ctx.registries,
    skipped: Array.from(ctx.skipped),
    store: ctx.storePath,
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
    .map((pair) => dp.refToRelative(pair[1], pair[0]))
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
  pkgsToRebuild: Set<string>,
  rootNodeModulesDir: string,
  lockfile: Lockfile,
  importers: Array<{ id: string, prefix: string }>,
  opts: StrictRebuildOptions,
) {
  const pkgsThatWereRebuilt = new Set()
  const graph = new Map()
  const pkgSnapshots: PackageSnapshots = lockfile.packages || {}

  const entryNodes = [] as string[]

  importers.forEach((importer) => {
    const lockfileImporter = lockfile.importers[importer.id]
    R.toPairs({
      ...(opts.development && lockfileImporter.devDependencies || {}),
      ...(opts.production && lockfileImporter.dependencies || {}),
      ...(opts.optional && lockfileImporter.optionalDependencies || {}),
    })
    .map((pair) => dp.refToRelative(pair[1], pair[0]))
    .filter((nodeId) => nodeId !== null)
    .forEach((relDepPath) => {
      entryNodes.push(relDepPath as string)
    })
  })

  const nodesToBuildAndTransitive = new Set()
  getSubgraphToBuild(pkgSnapshots, entryNodes, nodesToBuildAndTransitive, new Set(), { optional: opts.optional === true, pkgsToRebuild })
  const nodesToBuildAndTransitiveArray = Array.from(nodesToBuildAndTransitive)

  for (const relDepPath of nodesToBuildAndTransitiveArray) {
    const pkgSnapshot = pkgSnapshots[relDepPath]
    graph.set(relDepPath, R.toPairs({ ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })
      .map((pair) => dp.refToRelative(pair[1], pair[0]))
      .filter((childRelDepPath) => nodesToBuildAndTransitive.has(childRelDepPath)))
  }
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [nodesToBuildAndTransitiveArray],
  })
  const chunks = graphSequencerResult.chunks as string[][]
  const warn = (message: string) => logger.warn({ message, prefix: opts.prefix })
  const groups = chunks.map((chunk) => chunk.filter((relDepPath) => pkgsToRebuild.has(relDepPath)).map((relDepPath) =>
    async () => {
      const pkgSnapshot = pkgSnapshots[relDepPath]
      const depPath = dp.resolve(opts.registries, relDepPath)
      const pkgInfo = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
      const independent = opts.independentLeaves && packageIsIndependent(pkgSnapshot)
      const pkgRoot = !independent
        ? path.join(rootNodeModulesDir, `.${pkgIdToFilename(depPath, opts.lockfileDirectory)}`, 'node_modules', pkgInfo.name)
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
          const modules = path.join(rootNodeModulesDir, `.${pkgIdToFilename(depPath, opts.lockfileDirectory)}`, 'node_modules')
          const binPath = path.join(pkgRoot, 'node_modules', '.bin')
          await linkBins(modules, binPath, { warn })
        }
        await runPostinstallHooks({
          depPath,
          optional: pkgSnapshot.optional === true,
          pkgRoot,
          prepare: pkgSnapshot.prepare,
          rawNpmConfig: opts.rawNpmConfig,
          rootNodeModulesDir,
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
    R
      .keys(pkgSnapshots)
      .filter((relDepPath) => !packageIsIndependent(pkgSnapshots[relDepPath]))
      .map((relDepPath) => limitLinking(() => {
        const depPath = dp.resolve(opts.registries, relDepPath)
        const pkgSnapshot = pkgSnapshots[relDepPath]
        const pkgInfo = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
        const modules = path.join(rootNodeModulesDir, `.${pkgIdToFilename(depPath, opts.lockfileDirectory)}`, 'node_modules')
        const binPath = path.join(modules, pkgInfo.name, 'node_modules', '.bin')
        return linkBins(modules, binPath, { warn })
      })),
  )
  await Promise.all(importers.map((importer) => limitLinking(() => {
    const modules = path.join(importer.prefix, 'node_modules')
    const binPath = path.join(modules, '.bin')
    return linkBins(modules, binPath, { warn })
  })))

  return pkgsThatWereRebuilt
}
