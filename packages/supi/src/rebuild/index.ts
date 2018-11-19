import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import runLifecycleHooks, { runPostinstallHooks } from '@pnpm/lifecycle'
import logger, { streamParser } from '@pnpm/logger'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import {
  nameVerFromPkgSnapshot,
  PackageSnapshots,
  Shrinkwrap,
} from '@pnpm/shrinkwrap-utils'
import { PackageJson } from '@pnpm/types'
import npa = require('@zkochan/npm-package-arg')
import * as dp from 'dependency-path'
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
import path = require('path')
import R = require('ramda')
import semver = require('semver')
import { LAYOUT_VERSION } from '../constants'
import { getContextForSingleImporter } from '../getContext'
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
      const pkgShr = packages[relativeDepPath]
      const pkgInfo = nameVerFromPkgSnapshot(relativeDepPath, pkgShr)
      if (!pkgInfo.name) {
        logger.warn({
          message: `Skipping ${relativeDepPath} because cannot get the package name from shrinkwrap.yaml.
            Try to run run \`pnpm update --depth 100\` to create a new shrinkwrap.yaml with all the necessary info.`,
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
  pkgSpecs: string[],
  maybeOpts: RebuildOptions,
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContextForSingleImporter(opts)

  if (!ctx.currentShrinkwrap || !ctx.currentShrinkwrap.packages) return
  const packages = ctx.currentShrinkwrap.packages

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

  const pkgs = findPackages(packages, searched, { prefix: ctx.prefix })

  await _rebuild(new Set(pkgs), ctx.virtualStoreDir, ctx.currentShrinkwrap, ctx.importerId, opts)
}

export async function rebuild (maybeOpts: RebuildOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContextForSingleImporter(opts)

  let idsToRebuild: string[] = []

  if (opts.pending) {
    idsToRebuild = ctx.pendingBuilds
  } else if (ctx.currentShrinkwrap && ctx.currentShrinkwrap.packages) {
    idsToRebuild = R.keys(ctx.currentShrinkwrap.packages)
  } else {
    return
  }
  if (idsToRebuild.length === 0) return

  const pkgsThatWereRebuilt = await _rebuild(new Set(idsToRebuild), ctx.virtualStoreDir, ctx.currentShrinkwrap, ctx.importerId, opts)

  ctx.pendingBuilds = ctx.pendingBuilds.filter((relDepPath) => !pkgsThatWereRebuilt.has(relDepPath))

  if (ctx.pkg && ctx.pkg.scripts && (!opts.pending || ctx.pendingBuilds.indexOf(ctx.importerId) !== -1)) {
    await runLifecycleHooksInDir(opts.prefix, ctx.pkg, {
      rawNpmConfig: opts.rawNpmConfig,
      rootNodeModulesDir: ctx.modulesDir,
      unsafePerm: opts.unsafePerm,
    })

    ctx.pendingBuilds.splice(ctx.pendingBuilds.indexOf(ctx.importerId), 1)
  }

  await writeModulesYaml(ctx.virtualStoreDir, {
    ...ctx.modulesFile,
    importers: {
      ...ctx.modulesFile && ctx.modulesFile.importers,
      [ctx.importerId]: {
        hoistedAliases: ctx.hoistedAliases,
        shamefullyFlatten: opts.shamefullyFlatten,
      },
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

async function runLifecycleHooksInDir (
  prefix: string,
  pkg: PackageJson,
  opts: {
    rawNpmConfig: object,
    rootNodeModulesDir: string,
    unsafePerm: boolean,
  },
) {
  const scriptsOpts = {
    depPath: prefix,
    pkgRoot: prefix,
    rawNpmConfig: opts.rawNpmConfig,
    rootNodeModulesDir: opts.rootNodeModulesDir,
    unsafePerm: opts.unsafePerm || false,
  }
  if (pkg.scripts!.preinstall) {
    await runLifecycleHooks('preinstall', pkg, scriptsOpts)
  }
  if (pkg.scripts!.install) {
    await runLifecycleHooks('install', pkg, scriptsOpts)
  }
  if (pkg.scripts!.postinstall) {
    await runLifecycleHooks('postinstall', pkg, scriptsOpts)
  }
  if (pkg.scripts!.prepublish) {
    await runLifecycleHooks('prepublish', pkg, scriptsOpts)
  }
  if (pkg.scripts!.prepare) {
    await runLifecycleHooks('prepare', pkg, scriptsOpts)
  }
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
      logger.debug({ message: `No entry for "${depPath}" in shrinkwrap.yaml` })
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

async function _rebuild (
  pkgsToRebuild: Set<string>,
  modules: string,
  shr: Shrinkwrap,
  importerId: string,
  opts: StrictRebuildOptions,
) {
  const pkgsThatWereRebuilt = new Set()
  const limitChild = pLimit(opts.childConcurrency)
  const graph = new Map()
  const pkgSnapshots: PackageSnapshots = shr.packages || {}
  const shrImporter = shr.importers[importerId]

  const entryNodes = R.toPairs({
    ...(opts.development && shrImporter.devDependencies || {}),
    ...(opts.production && shrImporter.dependencies || {}),
    ...(opts.optional && shrImporter.optionalDependencies || {}),
  })
  .map((pair) => dp.refToRelative(pair[1], pair[0]))
  .filter((nodeId) => nodeId !== null) as string[]

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

  for (const chunk of chunks) {
    await Promise.all(chunk
      .filter((relDepPath) => pkgsToRebuild.has(relDepPath))
      .map((relDepPath) => {
        const pkgSnapshot = pkgSnapshots[relDepPath]
        return limitChild(async () => {
          const depAbsolutePath = dp.resolve(opts.registries.default, relDepPath)
          const pkgInfo = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
          try {
            await runPostinstallHooks({
              depPath: depAbsolutePath,
              pkgRoot: path.join(modules, `.${depAbsolutePath}`, 'node_modules', pkgInfo.name),
              prepare: pkgSnapshot.prepare,
              rawNpmConfig: opts.rawNpmConfig,
              rootNodeModulesDir: modules,
              unsafePerm: opts.unsafePerm || false,
            })
            pkgsThatWereRebuilt.add(relDepPath)
          } catch (err) {
            if (pkgSnapshot.optional) {
              // TODO: add parents field to the log
              skippedOptionalDependencyLogger.debug({
                details: err.toString(),
                package: {
                  id: pkgSnapshot.id || depAbsolutePath,
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
        })
      }),
    )
  }

  return pkgsThatWereRebuilt
}
