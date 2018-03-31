import {runPostinstallHooks} from '@pnpm/lifecycle'
import logger, {streamParser} from '@pnpm/logger'
import {write as writeModulesYaml} from '@pnpm/modules-yaml'
import npa = require('@zkochan/npm-package-arg')
import * as dp from 'dependency-path'
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
import path = require('path')
import {
  DependencyShrinkwrap,
  nameVerFromPkgSnapshot,
  PackageSnapshots,
  ResolvedPackages,
  Shrinkwrap,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import semver = require('semver')
import {LAYOUT_VERSION} from '../constants'
import realNodeModulesDir from '../fs/realNodeModulesDir'
import extendOptions, {
  RebuildOptions,
  StrictRebuildOptions,
} from './extendRebuildOptions'
import getContext from './getContext'

interface PackageToRebuild {
  relativeDepPath: string,
  name: string,
  version?: string,
  pkgShr: DependencyShrinkwrap
}

function findPackages (
  packages: ResolvedPackages,
  searched: PackageSelector[],
): string[] {
  return R.keys(packages)
    .filter((relativeDepPath) => {
      const pkgShr = packages[relativeDepPath]
      const pkgInfo = nameVerFromPkgSnapshot(relativeDepPath, pkgShr)
      if (!pkgInfo.name) {
        logger.warn(`Skipping ${relativeDepPath} because cannot get the package name from shrinkwrap.yaml.
          Try to run run \`pnpm update --depth 100\` to create a new shrinkwrap.yaml with all the necessary info.`)
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
  const ctx = await getContext(opts)
  const modules = await realNodeModulesDir(opts.prefix)

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

  const pkgs = findPackages(packages, searched)

  await _rebuild(new Set(pkgs), modules, ctx.currentShrinkwrap, opts)
}

export async function rebuild (maybeOpts: RebuildOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  const modules = await realNodeModulesDir(opts.prefix)

  let idsToRebuild: string[] = []

  if (opts.pending) {
    idsToRebuild = ctx.pendingBuilds
  } else if (ctx.currentShrinkwrap && ctx.currentShrinkwrap.packages) {
    idsToRebuild = R.keys(ctx.currentShrinkwrap.packages)
  } else {
    return
  }

  await _rebuild(new Set(idsToRebuild), modules, ctx.currentShrinkwrap, opts)

  await writeModulesYaml(path.join(ctx.root, 'node_modules'), {
    hoistedAliases: ctx.hoistedAliases,
    independentLeaves: opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: [],
    shamefullyFlatten: opts.shamefullyFlatten,
    skipped: Array.from(ctx.skipped),
    store: ctx.storePath,
  })
}

async function _rebuild (
  pkgsToRebuild: Set<string>,
  modules: string,
  shr: Shrinkwrap,
  opts: StrictRebuildOptions,
) {
  const limitChild = pLimit(opts.childConcurrency)
  const graph = new Map()
  const pkgSnapshots: PackageSnapshots = shr.packages || {}
  const relDepPaths = R.keys(pkgSnapshots)
  for (const relDepPath of relDepPaths) {
    const pkgSnapshot = pkgSnapshots[relDepPath]
    graph.set(relDepPath, R.toPairs({...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies}).map((pair) => {
      return dp.refToRelative(pair[1], pair[0])
    }))
  }
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [relDepPaths],
  })
  const chunks = graphSequencerResult.chunks as string[][]

  for (const chunk of chunks) {
    await Promise.all(chunk
      .filter((relDepPath) => pkgsToRebuild.has(relDepPath))
      .map((relDepPath) => {
        const pkgSnapshot = pkgSnapshots[relDepPath]
        return limitChild(async () => {
          const depAbsolutePath = dp.resolve(shr.registry, relDepPath)
          const pkgId = pkgSnapshot.id || depAbsolutePath
          try {
            const pkgInfo = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
            await runPostinstallHooks({
              pkgId,
              pkgRoot: path.join(modules, `.${depAbsolutePath}`, 'node_modules', pkgInfo.name),
              rawNpmConfig: opts.rawNpmConfig,
              rootNodeModulesDir: opts.prefix,
              unsafePerm: opts.unsafePerm || false,
            })
          } catch (err) {
            if (pkgSnapshot.optional) {
              logger.warn({
                err,
                message: `Skipping failed optional dependency ${pkgId}`,
              })
              return
            }
            throw err
          }
        })
      }),
    )
  }
}
