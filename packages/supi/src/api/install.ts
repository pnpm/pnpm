import headless, {HeadlessOptions} from '@pnpm/headless'
import runLifecycleHooks, {runPostinstallHooks} from '@pnpm/lifecycle'
import logger, {
  streamParser,
} from '@pnpm/logger'
import {write as writeModulesYaml} from '@pnpm/modules-yaml'
import {
  DirectoryResolution,
  Resolution,
} from '@pnpm/resolver-base'
import {
  Dependencies,
  PackageJson,
} from '@pnpm/types'
import {
  dependenciesTypes,
  getSaveType,
  packageJsonLogger,
  realNodeModulesDir,
  rootLogger,
  safeReadPackageFromDir as safeReadPkgFromDir,
  skippedOptionalDependencyLogger,
  stageLogger,
  summaryLogger,
} from '@pnpm/utils'
import * as dp from 'dependency-path'
import graphSequencer = require('graph-sequencer')
import pFilter = require('p-filter')
import pLimit = require('p-limit')
import {
  StoreController,
} from 'package-store'
import path = require('path')
import {
  satisfiesPackageJson,
  Shrinkwrap,
  write as saveShrinkwrap,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
  writeWantedOnly as saveWantedShrinkwrapOnly,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import {
  LAYOUT_VERSION,
  SHRINKWRAP_MINOR_VERSION,
} from '../constants'
import depsFromPackage, {getPreferredVersionsFromPackage} from '../depsFromPackage'
import depsToSpecs, {similarDepsToSpecs} from '../depsToSpecs'
import {absolutePathToRef} from '../fs/shrinkwrap'
import getSpecFromPackageJson from '../getSpecFromPackageJson'
import linkPackages, {DepGraphNodesByDepPath} from '../link'
import {DepGraphNode} from '../link/resolvePeers'
import {
  createNodeId,
  nodeIdContainsSequence,
  ROOT_NODE_ID,
} from '../nodeIdUtils'
import parseWantedDependencies from '../parseWantedDependencies'
import resolveDependencies, {Pkg} from '../resolveDependencies'
import safeIsInnerLink from '../safeIsInnerLink'
import save from '../save'
import {
  WantedDependency,
} from '../types'
import extendOptions, {
  InstallOptions,
  StrictInstallOptions,
} from './extendInstallOptions'
import getContext, {PnpmContext} from './getContext'
import externalLink from './link'
import lock from './lock'
import shrinkwrapsEqual from './shrinkwrapsEqual'
import getPref from './utils/getPref'

const ENGINE_NAME = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

export interface PkgByPkgId {
  [pkgId: string]: Pkg
}

export interface PkgGraphNode {
  children: (() => {[alias: string]: string}) | {[alias: string]: string}, // child nodeId by child alias name
  pkg: Pkg,
  depth: number,
  installable: boolean,
}

export interface PkgGraphNodeByNodeId {
  // a node ID is the join of the package's keypath with a colon
  // E.g., a subdeps node ID which parent is `foo` will be
  // registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
  [nodeId: string]: PkgGraphNode,
}

export interface InstallContext {
  defaultTag: string,
  dryRun: boolean,
  pkgByPkgId: PkgByPkgId,
  outdatedPkgs: {[pkgId: string]: string},
  localPackages: Array<{
    optional: boolean,
    dev: boolean,
    resolution: DirectoryResolution,
    id: string,
    version: string,
    name: string,
    specRaw: string,
    normalizedPref?: string,
    alias: string,
  }>,
  childrenByParentId: {[parentId: string]: Array<{alias: string, pkgId: string}>},
  nodesToBuild: Array<{
    alias: string,
    nodeId: string,
    pkg: Pkg,
    depth: number,
    installable: boolean,
  }>,
  wantedShrinkwrap: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap,
  storeController: StoreController,
  // the IDs of packages that are not installable
  skipped: Set<string>,
  pkgGraph: PkgGraphNodeByNodeId,
  force: boolean,
  prefix: string,
  registry: string,
  depth: number,
  engineStrict: boolean,
  nodeVersion: string,
  pnpmVersion: string,
  rawNpmConfig: object,
  nodeModules: string,
  verifyStoreInegrity: boolean,
  preferredVersions: {
    [packageName: string]: {
      type: 'version' | 'range' | 'tag',
      selector: string,
    },
  },
}

export async function install (maybeOpts: InstallOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const opts = await extendOptions(maybeOpts)

  if (!opts.production && opts.optional) {
    throw new Error('Optional dependencies cannot be installed without production dependencies')
  }

  if (opts.lock) {
    await lock(opts.prefix, _install, {
      locks: opts.locks,
      prefix: opts.prefix,
      stale: opts.lockStaleDuration,
    })
  } else {
    await _install()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function _install () {
    const installType = 'general'
    const ctx = await getContext(opts, installType)

    if (!ctx.pkg) throw new Error('No package.json found')

    if (!opts.update && (
      opts.frozenShrinkwrap ||
      opts.preferFrozenShrinkwrap && ctx.existsWantedShrinkwrap && ctx.wantedShrinkwrap.shrinkwrapMinorVersion === SHRINKWRAP_MINOR_VERSION &&
      !hasLocalTarballDepsInRoot(ctx.wantedShrinkwrap) &&
      satisfiesPackageJson(ctx.wantedShrinkwrap, ctx.pkg))
    ) {
      if (opts.shamefullyFlatten) {
        if (opts.frozenShrinkwrap) {
          logger.warn({
            message: 'Headless installation does not support flat node_modules layout yet',
            prefix: opts.prefix,
          })
        }
      } else if (!ctx.existsWantedShrinkwrap) {
        if (R.keys(ctx.pkg.dependencies).length || R.keys(ctx.pkg.devDependencies).length || R.keys(ctx.pkg.optionalDependencies).length) {
          throw new Error('Headless installation requires a shrinkwrap.yaml file')
        }
      } else {
        logger.info({message: 'Performing headless installation', prefix: ctx.prefix})
        await headless({
          ...opts,
          currentShrinkwrap: ctx.currentShrinkwrap,
          packageJson: ctx.pkg,
          wantedShrinkwrap: ctx.wantedShrinkwrap,
        } as HeadlessOptions)
        return
      }
    }

    const preferredVersions = getPreferredVersionsFromPackage(ctx.pkg)
    const specs = specsToInstallFromPackage(ctx.pkg, {
      prefix: opts.prefix,
    })

    if (ctx.wantedShrinkwrap.specifiers) {
      ctx.wantedShrinkwrap.dependencies = ctx.wantedShrinkwrap.dependencies || {}
      ctx.wantedShrinkwrap.devDependencies = ctx.wantedShrinkwrap.devDependencies || {}
      ctx.wantedShrinkwrap.optionalDependencies = ctx.wantedShrinkwrap.optionalDependencies || {}
      for (const spec of specs) {
        if (spec.alias && ctx.wantedShrinkwrap.specifiers[spec.alias] !== spec.pref) {
          if (ctx.wantedShrinkwrap.dependencies[spec.alias] && !ctx.wantedShrinkwrap.dependencies[spec.alias].startsWith('link:')) {
            delete ctx.wantedShrinkwrap.dependencies[spec.alias]
          }
          delete ctx.wantedShrinkwrap.devDependencies[spec.alias]
          delete ctx.wantedShrinkwrap.optionalDependencies[spec.alias]
        }
      }
    }

    const scripts = !opts.ignoreScripts && ctx.pkg && ctx.pkg.scripts || {}
    if (opts.ignoreScripts && ctx.pkg && ctx.pkg.scripts &&
      (ctx.pkg.scripts.preinstall || ctx.pkg.scripts.prepublish ||
        ctx.pkg.scripts.install ||
        ctx.pkg.scripts.postinstall ||
        ctx.pkg.scripts.prepublishOnly ||
        ctx.pkg.scripts.prepare)) {
          ctx.pendingBuilds.push('.')
        }

    if (scripts['prepublish']) { // tslint:disable-line:no-string-literal
      logger.warn({
        message: '`prepublish` scripts are deprecated. Use `prepare` for build steps and `prepublishOnly` for upload-only.',
        prefix: opts.prefix,
      })
    }

    const scriptsOpts = {
      depPath: opts.prefix,
      pkgRoot: opts.prefix,
      rawNpmConfig: opts.rawNpmConfig,
      rootNodeModulesDir: await realNodeModulesDir(opts.prefix),
      stdio: opts.ownLifecycleHooksStdio,
      unsafePerm: opts.unsafePerm || false,
    }

    if (scripts.preinstall) {
      await runLifecycleHooks('preinstall', ctx.pkg, scriptsOpts)
    }

    await installInContext(installType, specs, [], ctx, preferredVersions, opts)

    if (scripts.install) {
      await runLifecycleHooks('install', ctx.pkg, scriptsOpts)
    }
    if (scripts.postinstall) {
      await runLifecycleHooks('postinstall', ctx.pkg, scriptsOpts)
    }
    if (scripts.prepublish) {
      await runLifecycleHooks('prepublish', ctx.pkg, scriptsOpts)
    }
    if (scripts.prepare) {
      await runLifecycleHooks('prepare', ctx.pkg, scriptsOpts)
    }
  }
}

function hasLocalTarballDepsInRoot (shr: Shrinkwrap) {
  return R.any(refIsLocalTarball, R.values(shr.dependencies || {}))
    || R.any(refIsLocalTarball, R.values(shr.devDependencies || {}))
    || R.any(refIsLocalTarball, R.values(shr.optionalDependencies || {}))
}

function refIsLocalTarball (ref: string) {
  return ref.startsWith('file:') && (ref.endsWith('.tgz') || ref.endsWith('.tar.gz') || ref.endsWith('.tar'))
}

function specsToInstallFromPackage (
  pkg: PackageJson,
  opts: {
    prefix: string,
  },
): WantedDependency[] {
  const depsToInstall = depsFromPackage(pkg)
  return depsToSpecs(depsToInstall, {
    devDependencies: pkg.devDependencies || {},
    optionalDependencies: pkg.optionalDependencies || {},
  })
}

/**
 * Perform installation.
 *
 * @example
 *     install({'lodash': '1.0.0', 'foo': '^2.1.0' }, { silent: true })
 */
export async function installPkgs (
  fuzzyDeps: string[] | Dependencies,
  maybeOpts: InstallOptions,
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  if (maybeOpts.update === undefined) maybeOpts.update = true
  const opts = await extendOptions(maybeOpts)

  if (R.isEmpty(fuzzyDeps) && !opts.reinstallForFlatten) {
    throw new Error('At least one package has to be installed')
  }

  if (opts.lock) {
    await lock(opts.prefix, _installPkgs, {
      locks: opts.locks,
      prefix: opts.prefix,
      stale: opts.lockStaleDuration,
    })
  } else {
    await _installPkgs()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function _installPkgs () {
    const installType = 'named'
    const ctx = await getContext(opts, installType)
    const currentPrefs = opts.ignoreCurrentPrefs ? {} : depsFromPackage(ctx.pkg)
    const saveType = getSaveType(opts)
    const optionalDependencies = saveType ? {} : ctx.pkg.optionalDependencies || {}
    const devDependencies = saveType ? {} : ctx.pkg.devDependencies || {}
    const packagesToInstall = Array.isArray(fuzzyDeps)
      ? parseWantedDependencies(fuzzyDeps, {
        allowNew: opts.allowNew,
        currentPrefs,
        defaultTag: opts.tag,
        dev: opts.saveDev,
        devDependencies,
        optional: opts.saveOptional,
        optionalDependencies,
      })
      : similarDepsToSpecs(fuzzyDeps, {
        allowNew: opts.allowNew,
        currentPrefs,
        dev: opts.saveDev,
        devDependencies,
        optional: opts.saveOptional,
        optionalDependencies,
      })

    const preferredVersions = getPreferredVersionsFromPackage(ctx.pkg)
    return installInContext(
      installType,
      packagesToInstall,
      packagesToInstall.map((wantedDependency) => wantedDependency.raw),
      ctx,
      preferredVersions,
      opts)
  }
}

async function installInContext (
  installType: string,
  packagesToInstall: WantedDependency[],
  newPkgRawSpecs: string[],
  ctx: PnpmContext,
  preferredVersions: {
    [packageName: string]: {
      type: 'version' | 'range' | 'tag',
      selector: string,
    },
  },
  opts: StrictInstallOptions,
) {
  // Unfortunately, the private shrinkwrap file may differ from the public one.
  // A user might run named installations on a project that has a shrinkwrap.yaml file before running a noop install
  const makePartialCurrentShrinkwrap = installType === 'named' && (
    ctx.existsWantedShrinkwrap && !ctx.existsCurrentShrinkwrap ||
    // TODO: this operation is quite expensive. We'll have to find a better solution to do this.
    // maybe in pnpm v2 it won't be needed. See: https://github.com/pnpm/pnpm/issues/841
    !shrinkwrapsEqual(ctx.currentShrinkwrap, ctx.wantedShrinkwrap)
  )

  if (opts.shrinkwrapOnly && ctx.existsCurrentShrinkwrap) {
    logger.warn({
      message: '`node_modules` is present. Shrinkwrap only installation will make it out-of-date',
      prefix: ctx.prefix,
    })
  }

  const nodeModulesPath = await realNodeModulesDir(ctx.prefix)

  // Avoid requesting package meta info from registry only when the shrinkwrap version is at least the expected
  const hasManifestInShrinkwrap = typeof ctx.wantedShrinkwrap.shrinkwrapMinorVersion === 'number' &&
    ctx.wantedShrinkwrap.shrinkwrapMinorVersion >= SHRINKWRAP_MINOR_VERSION

  const installCtx: InstallContext = {
    childrenByParentId: {},
    currentShrinkwrap: ctx.currentShrinkwrap,
    defaultTag: opts.tag,
    depth: (() => {
      // This can be remove from shrinkwrap v4
      if (!hasManifestInShrinkwrap) {
        // The shrinkwrap file has to be updated to contain
        // the necessary info from package manifests
        return Infinity
      }
      if (opts.update) {
        return opts.depth
      }
      if (R.equals(ctx.wantedShrinkwrap.packages, ctx.currentShrinkwrap.packages)) {
        return opts.repeatInstallDepth
      }
      return Infinity
    })(),
    dryRun: opts.shrinkwrapOnly,
    engineStrict: opts.engineStrict,
    force: opts.force,
    localPackages: [],
    nodeModules: nodeModulesPath,
    nodeVersion: opts.nodeVersion,
    nodesToBuild: [],
    outdatedPkgs: {},
    pkgByPkgId: {},
    pkgGraph: {},
    pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
    preferredVersions,
    prefix: opts.prefix,
    rawNpmConfig: opts.rawNpmConfig,
    registry: ctx.wantedShrinkwrap.registry,
    skipped: ctx.skipped,
    storeController: opts.storeController,
    verifyStoreInegrity: opts.verifyStoreIntegrity,
    wantedShrinkwrap: ctx.wantedShrinkwrap,
  }
  const installOpts = {
    currentDepth: 0,
    hasManifestInShrinkwrap,
    keypath: [],
    localPackages: opts.localPackages,
    parentNodeId: ROOT_NODE_ID,
    readPackageHook: opts.hooks.readPackage,
    reinstallForFlatten: opts.reinstallForFlatten,
    resolvedDependencies: {
      ...ctx.wantedShrinkwrap.dependencies,
      ...ctx.wantedShrinkwrap.devDependencies,
      ...ctx.wantedShrinkwrap.optionalDependencies,
    },
    shamefullyFlatten: opts.shamefullyFlatten,
    sideEffectsCache: opts.sideEffectsCache,
    update: opts.update,
  }
  const nonLinkedPkgs: WantedDependency[] = []
  const linkedPkgs: Array<WantedDependency & {alias: string}> = []
  for (const wantedDependency of packagesToInstall) {
    if (!wantedDependency.alias) {
      nonLinkedPkgs.push(wantedDependency)
      continue
    }
    const isInnerLink = await safeIsInnerLink(nodeModulesPath, wantedDependency.alias, {
      prefix: ctx.prefix,
      storePath: ctx.storePath,
    })
    if (isInnerLink === true) {
      nonLinkedPkgs.push(wantedDependency)
      continue
    }
    // This info-log might be better to be moved to the reporter
    logger.info({
      message: `${wantedDependency.alias} is linked to ${nodeModulesPath} from ${isInnerLink}`,
      prefix: ctx.prefix,
    })
    linkedPkgs.push(wantedDependency as (WantedDependency & {alias: string}))
  }
  stageLogger.debug('resolution_started')
  const rootPkgs = await resolveDependencies(
    installCtx,
    nonLinkedPkgs,
    installOpts,
  )
  stageLogger.debug('resolution_done')
  installCtx.nodesToBuild.forEach((nodeToBuild) => {
    installCtx.pkgGraph[nodeToBuild.nodeId] = {
      children: () => buildTree(installCtx, nodeToBuild.nodeId, nodeToBuild.pkg.id,
        installCtx.childrenByParentId[nodeToBuild.pkg.id], nodeToBuild.depth + 1, nodeToBuild.installable),
      depth: nodeToBuild.depth,
      installable: nodeToBuild.installable,
      pkg: nodeToBuild.pkg,
    }
  })
  const rootNodeIdsByAlias = rootPkgs
    .reduce((acc, rootPkg) => {
      const pkg = installCtx.pkgGraph[rootPkg.nodeId].pkg
      const specRaw = pkg.specRaw
      const spec = R.find((sp) => sp.raw === specRaw, packagesToInstall)
      acc[rootPkg.alias] = rootPkg.nodeId
      return acc
    }, {})
  const pkgsToSave = (
    rootPkgs
      .map((rootPkg) => ({
        ...installCtx.pkgGraph[rootPkg.nodeId].pkg,
        alias: rootPkg.alias,
        normalizedPref: rootPkg.normalizedPref,
      })) as Array<{
        alias: string,
        optional: boolean,
        dev: boolean,
        resolution: Resolution,
        id: string,
        version: string,
        name: string,
        specRaw: string,
        normalizedPref?: string,
      }>)
  .concat(installCtx.localPackages)

  let newPkg: PackageJson | undefined = ctx.pkg
  if (installType === 'named' && !opts.reinstallForFlatten) {
    if (!ctx.pkg) {
      throw new Error('Cannot save because no package.json found')
    }
    const saveType = getSaveType(opts)
    const specsToUsert = <any>pkgsToSave // tslint:disable-line
      .map((dep) => {
        return {
          name: dep.alias,
          pref: dep.normalizedPref || getPref(dep.alias, dep.name, dep.version, {
            saveExact: opts.saveExact,
            savePrefix: opts.savePrefix,
          }),
          saveType,
        }
      })
    for (const pkgToInstall of packagesToInstall) {
      if (pkgToInstall.alias && !specsToUsert.some((spec: any) => spec.name === pkgToInstall.alias)) { // tslint:disable-line
        specsToUsert.push({
          name: pkgToInstall.alias,
          saveType,
        })
      }
    }
    const pkgJsonPath = path.join(ctx.prefix, 'package.json')
    newPkg = await save(
      ctx.prefix,
      specsToUsert,
    )
  } else {
    packageJsonLogger.debug({
      prefix: opts.prefix,
      updated: ctx.pkg,
    })
  }

  if (newPkg) {
    ctx.wantedShrinkwrap.dependencies = ctx.wantedShrinkwrap.dependencies || {}
    ctx.wantedShrinkwrap.specifiers = ctx.wantedShrinkwrap.specifiers || {}
    ctx.wantedShrinkwrap.optionalDependencies = ctx.wantedShrinkwrap.optionalDependencies || {}
    ctx.wantedShrinkwrap.devDependencies = ctx.wantedShrinkwrap.devDependencies || {}

    const devDeps = newPkg.devDependencies || {}
    const optionalDeps = newPkg.optionalDependencies || {}

    linkedPkgs.forEach((linkedPkg) => {
      ctx.wantedShrinkwrap.specifiers[linkedPkg.alias] = getSpecFromPackageJson(newPkg as PackageJson, linkedPkg.alias) as string
    })

    for (const dep of pkgsToSave) {
      const ref = absolutePathToRef(dep.id, {
        alias: dep.alias,
        realName: dep.name,
        resolution: dep.resolution,
        standardRegistry: ctx.wantedShrinkwrap.registry,
      })
      const isDev = !!devDeps[dep.alias]
      const isOptional = !!optionalDeps[dep.alias]
      if (isDev) {
        ctx.wantedShrinkwrap.devDependencies[dep.alias] = ref
      } else if (isOptional) {
        ctx.wantedShrinkwrap.optionalDependencies[dep.alias] = ref
      } else {
        ctx.wantedShrinkwrap.dependencies[dep.alias] = ref
      }
      if (!isDev) {
        delete ctx.wantedShrinkwrap.devDependencies[dep.alias]
      }
      if (!isOptional) {
        delete ctx.wantedShrinkwrap.optionalDependencies[dep.alias]
      }
      if (isDev || isOptional) {
        delete ctx.wantedShrinkwrap.dependencies[dep.alias]
      }
      ctx.wantedShrinkwrap.specifiers[dep.alias] = getSpecFromPackageJson(newPkg, dep.alias) as string
    }

    alignDependencyTypes(newPkg, ctx.wantedShrinkwrap)
  }

  const topParents = ctx.pkg
    ? await getTopParents(
        R.difference(
          R.keys(depsFromPackage(ctx.pkg)),
          newPkgRawSpecs && pkgsToSave.filter((pkgToSave) => newPkgRawSpecs.indexOf(pkgToSave.specRaw) !== -1).map((pkg) => pkg.alias) || [],
        ),
        nodeModulesPath,
      )
    : []

  const result = await linkPackages(rootNodeIdsByAlias, installCtx.pkgGraph, {
    afterAllResolvedHook: opts.hooks && opts.hooks.afterAllResolved,
    baseNodeModules: nodeModulesPath,
    bin: opts.bin,
    currentShrinkwrap: ctx.currentShrinkwrap,
    development: opts.development,
    dryRun: opts.shrinkwrapOnly,
    force: opts.force,
    hoistedAliases: ctx.hoistedAliases,
    independentLeaves: opts.independentLeaves,
    makePartialCurrentShrinkwrap,
    optional: opts.optional,
    outdatedPkgs: installCtx.outdatedPkgs,
    pkg: newPkg || ctx.pkg,
    prefix: ctx.prefix,
    production: opts.production,
    reinstallForFlatten: Boolean(opts.reinstallForFlatten),
    shamefullyFlatten: opts.shamefullyFlatten,
    sideEffectsCache: opts.sideEffectsCache,
    skipped: ctx.skipped,
    storeController: opts.storeController,
    topParents,
    updateShrinkwrapMinorVersion: installType === 'general' || R.isEmpty(ctx.currentShrinkwrap.packages),
    wantedShrinkwrap: ctx.wantedShrinkwrap,
  })
  ctx.hoistedAliases = result.hoistedAliases

  ctx.pendingBuilds = ctx.pendingBuilds
    .filter((relDepPath) => !result.removedDepPaths.has(dp.resolve(ctx.wantedShrinkwrap.registry, relDepPath)))

  if (opts.ignoreScripts) {
    // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
    ctx.pendingBuilds = ctx.pendingBuilds
      .concat(
        result.newDepPaths
          .filter((depPath) => result.depGraph[depPath].requiresBuild)
          .map((depPath) => dp.relative(ctx.wantedShrinkwrap.registry, depPath)),
      )
  }

  if (opts.shrinkwrapOnly) {
    await saveWantedShrinkwrapOnly(ctx.prefix, result.wantedShrinkwrap)
  } else {
    await Promise.all([
      opts.shrinkwrap
        ? saveShrinkwrap(ctx.prefix, result.wantedShrinkwrap, result.currentShrinkwrap)
        : saveCurrentShrinkwrapOnly(ctx.prefix, result.currentShrinkwrap),
      result.currentShrinkwrap.packages === undefined && result.removedDepPaths.size === 0
        ? Promise.resolve()
        : writeModulesYaml(path.join(ctx.prefix, 'node_modules'), {
          hoistedAliases: ctx.hoistedAliases,
          independentLeaves: opts.independentLeaves,
          layoutVersion: LAYOUT_VERSION,
          packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
          pendingBuilds: ctx.pendingBuilds,
          shamefullyFlatten: opts.shamefullyFlatten,
          skipped: Array.from(installCtx.skipped),
          store: ctx.storePath,
        }),
    ])

    // postinstall hooks
    if (!(opts.ignoreScripts || !result.newDepPaths || !result.newDepPaths.length)) {
      const limitChild = pLimit(opts.childConcurrency)

      const depPaths = Object.keys(result.depGraph)
      const rootNodes = depPaths.filter((depPath) => result.depGraph[depPath].depth === 0)
      const nodesToBuild = new Set<string>()
      getSubgraphToBuild(result.depGraph, rootNodes, nodesToBuild, new Set<string>())
      const onlyFromBuildGraph = R.filter((depPath: string) => nodesToBuild.has(depPath))

      const nodesToBuildArray = Array.from(nodesToBuild)
      const graph = new Map(
        nodesToBuildArray
          .map((depPath) => [depPath, onlyFromBuildGraph(R.values(result.depGraph[depPath].children))]) as Array<[string, string[]]>,
      )
      const graphSequencerResult = graphSequencer({
        graph,
        groups: [nodesToBuildArray],
      })
      const chunks = graphSequencerResult.chunks as string[][]

      for (const chunk of chunks) {
        await Promise.all(chunk
          .filter((depPath) => result.depGraph[depPath].requiresBuild && !result.depGraph[depPath].isBuilt && result.newDepPaths.indexOf(depPath) !== -1)
          .map((depPath) => result.depGraph[depPath])
          .map((pkg) => limitChild(async () => {
            try {
              const hasSideEffects = await runPostinstallHooks({
                depPath: pkg.absolutePath,
                pkgRoot: pkg.peripheralLocation,
                prepare: pkg.prepare,
                rawNpmConfig: installCtx.rawNpmConfig,
                rootNodeModulesDir: nodeModulesPath,
                unsafePerm: opts.unsafePerm || false,
              })
              if (hasSideEffects && opts.sideEffectsCache && !opts.sideEffectsCacheReadonly) {
                try {
                  await installCtx.storeController.upload(pkg.peripheralLocation, {
                    engine: ENGINE_NAME,
                    pkgId: pkg.id,
                  })
                } catch (err) {
                  if (err && err.statusCode === 403) {
                    logger.warn({
                      message: `The store server disabled upload requests, could not upload ${pkg.id}`,
                      prefix: ctx.prefix,
                    })
                  } else {
                    logger.warn({
                      error: err,
                      message: `An error occurred while uploading ${pkg.id}`,
                      prefix: ctx.prefix,
                    })
                  }
                }
              }
            } catch (err) {
              if (installCtx.pkgByPkgId[pkg.id].optional) {
                // TODO: add parents field to the log
                skippedOptionalDependencyLogger.debug({
                  details: err.toString(),
                  package: {
                    id: pkg.id,
                    name: pkg.name,
                    version: pkg.version,
                  },
                  prefix: opts.prefix,
                  reason: 'build_failure',
                })
                return
              }
              throw err
            }
          },
        )))
      }
    }

    // TODO: link inside resolveDependencies.ts
    if (installCtx.localPackages.length) {
      const linkOpts = {
        ...opts,
        linkToBin: opts.bin,
        saveDev: false,
        saveOptional: false,
        saveProd: false,
        skipInstall: true,
      }
      const externalPkgs = installCtx.localPackages.map((localPackage) => resolvePath(opts.prefix, localPackage.resolution.directory))
      await externalLink(externalPkgs, installCtx.nodeModules, linkOpts)
    }
  }

  // waiting till the skipped packages are downloaded to the store
  await Promise.all(
    R.props<string, Pkg>(Array.from(installCtx.skipped), installCtx.pkgByPkgId)
      // skipped packages might have not been reanalized on a repeat install
      // so lets just ignore those by excluding nulls
      .filter(Boolean)
      .map((pkg) => pkg.fetchingFiles),
  )

  // waiting till package requests are finished
  await Promise.all(R.values(installCtx.pkgByPkgId).map((installed) => installed.finishing))

  summaryLogger.debug({prefix: opts.prefix})

  await opts.storeController.close()
}

const isAbsolutePath = /^[/]|^[A-Za-z]:/

// This function is copied from @pnpm/local-resolver
function resolvePath (where: string, spec: string) {
  if (isAbsolutePath.test(spec)) return spec
  return path.resolve(where, spec)
}

function getSubgraphToBuild (
  graph: DepGraphNodesByDepPath,
  entryNodes: string[],
  nodesToBuild: Set<string>,
  walked: Set<string>,
) {
  let currentShouldBeBuilt = false
  for (const depPath of entryNodes) {
    if (nodesToBuild.has(depPath)) {
      currentShouldBeBuilt = true
    }
    if (walked.has(depPath)) continue
    walked.add(depPath)
    const childShouldBeBuilt = getSubgraphToBuild(graph, R.values(graph[depPath].children), nodesToBuild, walked)
      || graph[depPath].requiresBuild
    if (childShouldBeBuilt) {
      nodesToBuild.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  return currentShouldBeBuilt
}

function alignDependencyTypes (pkg: PackageJson, shr: Shrinkwrap) {
  const depTypesOfAliases = getAliasToDependencyTypeMap(pkg)

  // Aligning the dependency types in shrinkwrap.yaml
  for (const depType of dependenciesTypes) {
    if (!shr[depType]) continue
    for (const alias of Object.keys(shr[depType] || {})) {
      if (depType === depTypesOfAliases[alias] || !depTypesOfAliases[alias]) continue
      shr[depTypesOfAliases[alias]][alias] = shr[depType]![alias]
      delete shr[depType]![alias]
    }
  }
}

function getAliasToDependencyTypeMap (pkg: PackageJson) {
  const depTypesOfAliases = {}
  for (const depType of dependenciesTypes) {
    if (!pkg[depType]) continue
    for (const alias of Object.keys(pkg[depType] || {})) {
      if (!depTypesOfAliases[alias]) {
        depTypesOfAliases[alias] = depType
      }
    }
  }
  return depTypesOfAliases
}

function buildTree (
  ctx: InstallContext,
  parentNodeId: string,
  parentId: string,
  children: Array<{alias: string, pkgId: string}>,
  depth: number,
  installable: boolean,
) {
  const childrenNodeIds = {}
  for (const child of children) {
    if (nodeIdContainsSequence(parentNodeId, parentId, child.pkgId)) {
      continue
    }
    const childNodeId = createNodeId(parentNodeId, child.pkgId)
    childrenNodeIds[child.alias] = childNodeId
    installable = installable && !ctx.skipped.has(child.pkgId)
    ctx.pkgGraph[childNodeId] = {
      children: () => buildTree(ctx, childNodeId, child.pkgId, ctx.childrenByParentId[child.pkgId], depth + 1, installable),
      depth,
      installable,
      pkg: ctx.pkgByPkgId[child.pkgId],
    }
  }
  return childrenNodeIds
}

async function getTopParents (pkgNames: string[], modules: string) {
  const pkgs = await Promise.all(
    pkgNames.map((pkgName) => path.join(modules, pkgName)).map(safeReadPkgFromDir),
  )
  return pkgs.filter(Boolean).map((pkg: PackageJson) => ({
    name: pkg.name,
    version: pkg.version,
  }))
}
