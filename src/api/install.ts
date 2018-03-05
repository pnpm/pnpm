import {
  Dependencies,
  PackageJson,
} from '@pnpm/types'
import * as dp from 'dependency-path'
import path = require('path')
import logger, {
  streamParser,
} from '@pnpm/logger'
import {
  stageLogger,
  summaryLogger,
  packageJsonLogger,
  rootLogger,
} from '../loggers'
import logStatus from '../logging/logInstallStatus'
import pLimit = require('p-limit')
import pFilter = require('p-filter')
import R = require('ramda')
import safeIsInnerLink from '../safeIsInnerLink'
import {fromDir as safeReadPkgFromDir} from '../fs/safeReadPkg'
import {
  WantedDependency,
} from '../types'
import getContext, {PnpmContext} from './getContext'
import resolveDependencies, {InstalledPackage} from '../resolveDependencies'
import externalLink from './link'
import linkPackages from '../link'
import save from '../save'
import getSaveType from '../getSaveType'
import postInstall, {npmRunScript} from '../install/postInstall'
import extendOptions, {
  InstallOptions,
  StrictInstallOptions,
} from './extendInstallOptions'
import lock from './lock'
import {
  write as saveShrinkwrap,
  writeWantedOnly as saveWantedShrinkwrapOnly,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
  Shrinkwrap,
} from 'pnpm-shrinkwrap'
import {absolutePathToRef} from '../fs/shrinkwrap'
import {
  save as saveModules,
  LAYOUT_VERSION,
} from '../fs/modulesController'
import {DependencyTreeNode} from '../link/resolvePeers'
import depsToSpecs, {similarDepsToSpecs} from '../depsToSpecs'
import shrinkwrapsEqual from './shrinkwrapsEqual'
import {
  StoreController,
} from 'package-store'
import depsFromPackage, {getPreferredVersionsFromPackage} from '../depsFromPackage'
import parseWantedDependencies from '../parseWantedDependencies'
import {
  DirectoryResolution,
  Resolution,
} from '@pnpm/resolver-base'
import {
  nodeIdContainsSequence,
  createNodeId,
  ROOT_NODE_ID,
} from '../nodeIdUtils'
import realNodeModulesDir from '../fs/realNodeModulesDir'
import getSpecFromPackageJson from '../getSpecFromPackageJson'

const ENGINE_NAME = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

export type InstalledPackages = {
  [name: string]: InstalledPackage
}

export type TreeNode = {
  children: (() => {[alias: string]: string}) | {[alias: string]: string}, // child nodeId by child alias name
  pkg: InstalledPackage,
  depth: number,
  installable: boolean,
}

export type TreeNodeMap = {
  [nodeId: string]: TreeNode,
}

export type InstallContext = {
  defaultTag: string,
  dryRun: boolean,
  installs: InstalledPackages,
  outdatedPkgs: {[pkgId: string]: string},
  localPackages: {
    optional: boolean,
    dev: boolean,
    resolution: DirectoryResolution,
    id: string,
    version: string,
    name: string,
    specRaw: string,
    normalizedPref?: string,
    alias: string,
  }[],
  childrenByParentId: {[parentId: string]: {alias: string, pkgId: string}[]},
  nodesToBuild: {
    alias: string,
    nodeId: string,
    pkg: InstalledPackage,
    depth: number,
    installable: boolean,
  }[],
  wantedShrinkwrap: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap,
  storeController: StoreController,
  // the IDs of packages that are not installable
  skipped: Set<string>,
  tree: {[nodeId: string]: TreeNode},
  force: boolean,
  prefix: string,
  registry: string,
  depth: number,
  engineStrict: boolean,
  nodeVersion: string,
  pnpmVersion: string,
  rawNpmConfig: Object,
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
    await lock(opts.prefix, _install, {stale: opts.lockStaleDuration, locks: opts.locks})
  } else {
    await _install()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function _install() {
    const installType = 'general'
    const ctx = await getContext(opts, installType)

    if (!ctx.pkg) throw new Error('No package.json found')

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

    if (scripts['prepublish']) {
      logger.warn('`prepublish` scripts are deprecated. Use `prepare` for build steps and `prepublishOnly` for upload-only.')
    }

    const scriptsOpts = {
      rawNpmConfig: opts.rawNpmConfig,
      modulesDir: await realNodeModulesDir(opts.prefix),
      root: opts.prefix,
      pkgId: opts.prefix,
      stdio: 'inherit',
      unsafePerm: opts.unsafePerm || false,
    }

    if (scripts['preinstall']) {
      await npmRunScript('preinstall', ctx.pkg, scriptsOpts)
    }

    await installInContext(installType, specs, [], ctx, preferredVersions, opts)

    if (scripts['install']) {
      await npmRunScript('install', ctx.pkg, scriptsOpts)
    }
    if (scripts['postinstall']) {
      await npmRunScript('postinstall', ctx.pkg, scriptsOpts)
    }
    if (scripts['prepublish']) {
      await npmRunScript('prepublish', ctx.pkg, scriptsOpts)
    }
    if (scripts['prepare']) {
      await npmRunScript('prepare', ctx.pkg, scriptsOpts)
    }
  }
}

function specsToInstallFromPackage(
  pkg: PackageJson,
  opts: {
    prefix: string,
  }
): WantedDependency[] {
  const depsToInstall = depsFromPackage(pkg)
  return depsToSpecs(depsToInstall, {
    optionalDependencies: pkg.optionalDependencies || {},
    devDependencies: pkg.devDependencies || {},
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

  if (opts.lock) {
    await lock(opts.prefix, _installPkgs, {stale: opts.lockStaleDuration, locks: opts.locks})
  } else {
    await _installPkgs()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function _installPkgs () {
    const installType = 'named'
    const ctx = await getContext(opts, installType)
    const currentPrefs = opts.global ? {} : depsFromPackage(ctx.pkg)
    const saveType = getSaveType(opts)
    const optionalDependencies = saveType ? {} : ctx.pkg.optionalDependencies || {}
    const devDependencies = saveType ? {} : ctx.pkg.devDependencies || {}
    let packagesToInstall = Array.isArray(fuzzyDeps)
      ? parseWantedDependencies(fuzzyDeps, {
        defaultTag: opts.tag,
        dev: opts.saveDev,
        optional: opts.saveOptional,
        currentPrefs,
        optionalDependencies,
        devDependencies,
      })
      : similarDepsToSpecs(fuzzyDeps, {
        dev: opts.saveDev,
        optional: opts.saveOptional,
        currentPrefs,
        optionalDependencies,
        devDependencies,
      })

    if (!Object.keys(packagesToInstall).length && !opts.reinstallForFlatten) {
      throw new Error('At least one package has to be installed')
    }

    const preferredVersions = getPreferredVersionsFromPackage(ctx.pkg)
    return installInContext(
      installType,
      packagesToInstall,
      packagesToInstall.map(wantedDependency => wantedDependency.raw),
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
    logger.warn('`node_modules` is present. Shrinkwrap only installation will make it out-of-date')
  }

  const nodeModulesPath = await realNodeModulesDir(ctx.root)

  // This works from minor version 1, so any number is fine
  // also, the shrinkwrapMinorVersion is going to be removed from shrinkwrap v4
  const hasManifestInShrinkwrap = typeof ctx.wantedShrinkwrap.shrinkwrapMinorVersion === 'number'

  const installCtx: InstallContext = {
    defaultTag: opts.tag,
    dryRun: opts.shrinkwrapOnly,
    installs: {},
    outdatedPkgs: {},
    localPackages: [],
    childrenByParentId: {},
    nodesToBuild: [],
    wantedShrinkwrap: ctx.wantedShrinkwrap,
    currentShrinkwrap: ctx.currentShrinkwrap,
    skipped: ctx.skipped,
    tree: {},
    registry: ctx.wantedShrinkwrap.registry,
    force: opts.force,
    depth: (function () {
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
    prefix: opts.prefix,
    rawNpmConfig: opts.rawNpmConfig,
    nodeModules: nodeModulesPath,
    verifyStoreInegrity: opts.verifyStoreIntegrity,
    engineStrict: opts.engineStrict,
    nodeVersion: opts.nodeVersion,
    pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
    storeController: opts.storeController,
    preferredVersions,
  }
  const installOpts = {
    root: ctx.root,
    resolvedDependencies: {
      ...ctx.wantedShrinkwrap.devDependencies,
      ...ctx.wantedShrinkwrap.dependencies,
      ...ctx.wantedShrinkwrap.optionalDependencies
    },
    update: opts.update,
    keypath: [],
    parentNodeId: ROOT_NODE_ID,
    currentDepth: 0,
    readPackageHook: opts.hooks.readPackage,
    hasManifestInShrinkwrap,
    sideEffectsCache: opts.sideEffectsCache,
    reinstallForFlatten: opts.reinstallForFlatten,
    shamefullyFlatten: opts.shamefullyFlatten,
  }
  const nonLinkedPkgs: WantedDependency[] = []
  const linkedPkgs: (WantedDependency & {alias: string})[] = []
  for (const wantedDependency of packagesToInstall) {
    if (!wantedDependency.alias) {
      nonLinkedPkgs.push(wantedDependency)
      continue
    }
    const isInnerLink = await safeIsInnerLink(nodeModulesPath, wantedDependency.alias, {
      storePath: ctx.storePath,
    })
    if (isInnerLink === true) {
      nonLinkedPkgs.push(wantedDependency)
      continue
    }
    rootLogger.debug({
      linked: {
        name: wantedDependency.alias,
        from: isInnerLink as string,
        to: nodeModulesPath,
        dependencyType: wantedDependency.dev && 'dev' || wantedDependency.optional && 'optional' || 'prod',
      },
    })
    // This info-log might be better to be moved to the reporter
    logger.info(`${wantedDependency.alias} is linked to ${nodeModulesPath} from ${isInnerLink}`)
    linkedPkgs.push(wantedDependency as (WantedDependency & {alias: string}))
  }
  stageLogger.debug('resolution_started')
  const rootPkgs = await resolveDependencies(
    installCtx,
    nonLinkedPkgs,
    installOpts
  )
  stageLogger.debug('resolution_done')
  installCtx.nodesToBuild.forEach(nodeToBuild => {
    installCtx.tree[nodeToBuild.nodeId] = {
      pkg: nodeToBuild.pkg,
      children: () => buildTree(installCtx, nodeToBuild.nodeId, nodeToBuild.pkg.id,
        installCtx.childrenByParentId[nodeToBuild.pkg.id], nodeToBuild.depth + 1, nodeToBuild.installable),
      depth: nodeToBuild.depth,
      installable: nodeToBuild.installable,
    }
  })
  const rootNodeIdsByAlias = rootPkgs
    .reduce((rootNodeIdsByAlias, rootPkg) => {
      const pkg = installCtx.tree[rootPkg.nodeId].pkg
      const specRaw = pkg.specRaw
      const spec = R.find(spec => spec.raw === specRaw, packagesToInstall)
      rootNodeIdsByAlias[rootPkg.alias] = rootPkg.nodeId
      return rootNodeIdsByAlias
    }, {})
  const pkgsToSave = (
    rootPkgs
      .map(rootPkg => ({
        ...installCtx.tree[rootPkg.nodeId].pkg,
        alias: rootPkg.alias,
        normalizedPref: rootPkg.normalizedPref,
      })) as {
        alias: string,
        optional: boolean,
        dev: boolean,
        resolution: Resolution,
        id: string,
        version: string,
        name: string,
        specRaw: string,
        normalizedPref?: string,
      }[])
  .concat(installCtx.localPackages)

  let newPkg: PackageJson | undefined = ctx.pkg
  if (installType === 'named' && !opts.reinstallForFlatten) {
    if (!ctx.pkg) {
      throw new Error('Cannot save because no package.json found')
    }
    const pkgJsonPath = path.join(ctx.root, 'package.json')
    const saveType = getSaveType(opts)
    newPkg = await save(
      pkgJsonPath,
      <any>pkgsToSave // tslint:disable-line
        .map(dep => {
          return {
            name: dep.alias,
            pref: dep.normalizedPref || getPref(dep.alias, dep.name, dep.version, {
              saveExact: opts.saveExact,
              savePrefix: opts.savePrefix,
            })
          }
        }),
      saveType
    )
  } else {
    packageJsonLogger.debug({ updated: ctx.pkg })
  }

  if (newPkg) {
    ctx.wantedShrinkwrap.dependencies = ctx.wantedShrinkwrap.dependencies || {}
    ctx.wantedShrinkwrap.specifiers = ctx.wantedShrinkwrap.specifiers || {}
    ctx.wantedShrinkwrap.optionalDependencies = ctx.wantedShrinkwrap.optionalDependencies || {}
    ctx.wantedShrinkwrap.devDependencies = ctx.wantedShrinkwrap.devDependencies || {}

    const devDeps = newPkg.devDependencies || {}
    const optionalDeps = newPkg.optionalDependencies || {}

    linkedPkgs.forEach(linkedPkg => {
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
  }

  const topParents = ctx.pkg
    ? await getTopParents(
        R.difference(
          R.keys(depsFromPackage(ctx.pkg)),
          newPkgRawSpecs && pkgsToSave.filter(pkgToSave => newPkgRawSpecs.indexOf(pkgToSave.specRaw) !== -1).map(pkg => pkg.alias) || []
        ),
        nodeModulesPath
      )
    : []

  const result = await linkPackages(rootNodeIdsByAlias, installCtx.tree, {
    dryRun: opts.shrinkwrapOnly,
    force: opts.force,
    global: opts.global,
    baseNodeModules: nodeModulesPath,
    bin: opts.bin,
    topParents,
    wantedShrinkwrap: ctx.wantedShrinkwrap,
    production: opts.production,
    development: opts.development,
    optional: opts.optional,
    root: ctx.root,
    currentShrinkwrap: ctx.currentShrinkwrap,
    skipped: ctx.skipped,
    pkg: newPkg || ctx.pkg,
    independentLeaves: opts.independentLeaves,
    storeController: opts.storeController,
    makePartialCurrentShrinkwrap,
    updateShrinkwrapMinorVersion: installType === 'general' || R.isEmpty(ctx.currentShrinkwrap.packages),
    outdatedPkgs: installCtx.outdatedPkgs,
    sideEffectsCache: opts.sideEffectsCache,
    shamefullyFlatten: opts.shamefullyFlatten,
    reinstallForFlatten: Boolean(opts.reinstallForFlatten),
    hoistedAliases: ctx.hoistedAliases,
  })
  ctx.hoistedAliases = result.hoistedAliases

  ctx.pendingBuilds = ctx.pendingBuilds
    .filter(pkgId => !result.removedPkgIds.has(dp.resolve(ctx.wantedShrinkwrap.registry, pkgId)))

  if (opts.ignoreScripts) {
    // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
    ctx.pendingBuilds = ctx.pendingBuilds
      .concat(result.newDepPaths.map(depPath => dp.relative(ctx.wantedShrinkwrap.registry, depPath)))
  }

  if (opts.shrinkwrapOnly) {
    await saveWantedShrinkwrapOnly(ctx.root, result.wantedShrinkwrap)
  } else {
    await Promise.all([
      opts.shrinkwrap
        ? saveShrinkwrap(ctx.root, result.wantedShrinkwrap, result.currentShrinkwrap)
        : saveCurrentShrinkwrapOnly(ctx.root, result.currentShrinkwrap),
      result.currentShrinkwrap.packages === undefined && result.removedPkgIds.size === 0
        ? Promise.resolve()
        : saveModules(path.join(ctx.root, 'node_modules'), {
          packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
          store: ctx.storePath,
          skipped: Array.from(installCtx.skipped),
          layoutVersion: LAYOUT_VERSION,
          independentLeaves: opts.independentLeaves,
          pendingBuilds: ctx.pendingBuilds,
          shamefullyFlatten: opts.shamefullyFlatten,
          hoistedAliases: ctx.hoistedAliases,
        }),
    ])

    // postinstall hooks
    if (!(opts.ignoreScripts || !result.newDepPaths || !result.newDepPaths.length)) {
      const limitChild = pLimit(opts.childConcurrency)
      await Promise.all(
        R.props<string, DependencyTreeNode>(result.newDepPaths, result.linkedPkgsMap)
          .filter(pkg => !pkg.isBuilt)
          .map(pkg => limitChild(async () => {
            try {
              const hasSideEffects = await postInstall(pkg.peripheralLocation, {
                rawNpmConfig: installCtx.rawNpmConfig,
                initialWD: ctx.root,
                userAgent: opts.userAgent,
                pkgId: pkg.id,
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
                    logger.warn(`The store server disabled upload requests, could not upload ${pkg.id}`)
                  } else {
                    logger.warn({
                      message: `An error occurred while uploading ${pkg.id}`,
                      err,
                    })
                  }
                }
              }
            } catch (err) {
              if (installCtx.installs[pkg.id].optional) {
                logger.warn({
                  message: `Skipping failed optional dependency ${pkg.id}`,
                  err,
                })
                return
              }
              throw err
            }
          })
        )
      )
    }

    if (installCtx.localPackages.length) {
      const linkOpts = {
        ...opts,
        skipInstall: true,
        linkToBin: opts.bin,
      }
      const externalPkgs = installCtx.localPackages.map(localPackage => localPackage.resolution.directory)
      await externalLink(externalPkgs, installCtx.nodeModules, linkOpts)
      installCtx.localPackages.forEach(async localPackage => {
        logStatus({
          status: 'installed',
          pkgId: localPackage.id,
        })
      })
    }
  }

  // waiting till the skipped packages are downloaded to the store
  await Promise.all(
    R.props<string, InstalledPackage>(Array.from(installCtx.skipped), installCtx.installs)
      // skipped packages might have not been reanalized on a repeat install
      // so lets just ignore those by excluding nulls
      .filter(Boolean)
      .map(pkg => pkg.fetchingFiles)
  )

  // waiting till package requests are finished
  await Promise.all(R.values(installCtx.installs).map(installed => installed.finishing))

  summaryLogger.info(undefined)

  await opts.storeController.close()
}

function buildTree (
  ctx: InstallContext,
  parentNodeId: string,
  parentId: string,
  children: {alias: string, pkgId: string}[],
  depth: number,
  installable: boolean
) {
  const childrenNodeIds = {}
  for (const child of children) {
    if (nodeIdContainsSequence(parentNodeId, parentId, child.pkgId)) {
      continue
    }
    const childNodeId = createNodeId(parentNodeId, child.pkgId)
    childrenNodeIds[child.alias] = childNodeId
    installable = installable && !ctx.skipped.has(child.pkgId)
    ctx.tree[childNodeId] = {
      pkg: ctx.installs[child.pkgId],
      children: () => buildTree(ctx, childNodeId, child.pkgId, ctx.childrenByParentId[child.pkgId], depth + 1, installable),
      depth,
      installable,
    }
  }
  return childrenNodeIds
}

async function getTopParents (pkgNames: string[], modules: string) {
  const pkgs = await Promise.all(
    pkgNames.map(pkgName => path.join(modules, pkgName)).map(safeReadPkgFromDir)
  )
  return pkgs.filter(Boolean).map((pkg: PackageJson) => ({
    name: pkg.name,
    version: pkg.version,
  }))
}

function getPref (
  alias: string,
  name: string,
  version: string,
  opts: {
    saveExact: boolean,
    savePrefix: string,
  }
) {
  let prefix = alias !== name ? `npm:${name}@` : ''
  if (opts.saveExact) return `${prefix}${version}`
  return `${prefix}${opts.savePrefix}${version}`
}
