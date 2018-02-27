import extendOptions, {
  RebuildOptions,
  StrictRebuildOptions,
} from './extendRebuildOptions'
import getContext from './getContext'
import logger, {streamParser} from '@pnpm/logger'
import R = require('ramda')
import * as dp from 'dependency-path'
import postInstall from '../install/postInstall'
import path = require('path')
import pSeries = require('p-series')
import {
  ResolvedPackages,
  DependencyShrinkwrap,
} from 'pnpm-shrinkwrap'
import npa = require('@zkochan/npm-package-arg')
import semver = require('semver')
import getPkgInfoFromShr from '../getPkgInfoFromShr'
import {save as saveModules, LAYOUT_VERSION} from '../fs/modulesController';
import realNodeModulesDir from '../fs/realNodeModulesDir';

type PackageToRebuild = {
  relativeDepPath: string,
  name: string,
  version?: string,
  pkgShr: DependencyShrinkwrap
}

function getPackagesInfo (packages: ResolvedPackages, idsToRebuild: string[]): PackageToRebuild[] {
  return idsToRebuild
    .map(relativeDepPath => {
      const pkgShr = packages[relativeDepPath]
      const pkgInfo = getPkgInfoFromShr(relativeDepPath, pkgShr)
      return {
        relativeDepPath,
        name: pkgInfo['name'],
        version: pkgInfo['version'],
        pkgShr,
      }
    })
    .filter(pkgInfo => {
      if (!pkgInfo.name) {
        logger.warn(`Skipping ${pkgInfo.relativeDepPath} because cannot get the package name from shrinkwrap.yaml.
          Try to run run \`pnpm update --depth 100\` to create a new shrinkwrap.yaml with all the necessary info.`)
        return false
      }
      return true
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

  const searched: PackageSelector[] = pkgSpecs.map(arg => {
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

  const pkgs = getPackagesInfo(packages, R.keys(packages))
    .filter(pkg => matches(searched, pkg))

  await _rebuild(pkgs, modules, ctx.currentShrinkwrap.registry, opts)
}

// TODO: move this logic to separate package as this is also used in dependencies-hierarchy
function matches (
  searched: PackageSelector[],
  pkg: {name: string, version?: string}
) {
  return searched.some(searchedPkg => {
    if (typeof searchedPkg === 'string') {
      return pkg.name === searchedPkg
    }
    return searchedPkg.name === pkg.name && !!pkg.version &&
      semver.satisfies(pkg.version, searchedPkg.range)
  })
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

  const pkgs = getPackagesInfo(ctx.currentShrinkwrap.packages || {}, idsToRebuild)

  await _rebuild(pkgs, modules, ctx.currentShrinkwrap.registry, opts)

  await saveModules(path.join(ctx.root, 'node_modules'), {
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    store: ctx.storePath,
    skipped: Array.from(ctx.skipped),
    layoutVersion: LAYOUT_VERSION,
    independentLeaves: opts.independentLeaves,
    pendingBuilds: [],
    shamefullyFlatten: opts.shamefullyFlatten,
    hoistedAliases: ctx.hoistedAliases,
  })
}

async function _rebuild (
  pkgs: PackageToRebuild[],
  modules: string,
  registry: string,
  opts: StrictRebuildOptions,
) {
  await pSeries(
    pkgs
      .map(pkgToRebuild => async () => {
        const depAbsolutePath = dp.resolve(registry, pkgToRebuild.relativeDepPath)
        const pkgId = pkgToRebuild.pkgShr.id || depAbsolutePath
        try {
          await postInstall(path.join(modules, `.${depAbsolutePath}`, 'node_modules', pkgToRebuild.name), {
            rawNpmConfig: opts.rawNpmConfig,
            initialWD: opts.prefix,
            userAgent: opts.userAgent,
            pkgId,
            unsafePerm: opts.unsafePerm || false,
          })
        } catch (err) {
          if (pkgToRebuild.pkgShr.optional) {
            logger.warn({
              message: `Skipping failed optional dependency ${pkgId}`,
              err,
            })
            return
          }
          throw err
        }
      })
  )
}
