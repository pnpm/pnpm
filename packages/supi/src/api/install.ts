import {
  packageJsonLogger,
  skippedOptionalDependencyLogger,
  stageLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import headless, { HeadlessOptions } from '@pnpm/headless'
import runLifecycleHooks, { runPostinstallHooks } from '@pnpm/lifecycle'
import logger, {
  streamParser,
} from '@pnpm/logger'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import {
  DirectoryResolution,
  LocalPackages,
  Resolution,
} from '@pnpm/resolver-base'
import {
  Dependencies,
  DEPENDENCIES_FIELDS,
  PackageJson,
} from '@pnpm/types'
import {
  getSaveType,
  realNodeModulesDir,
  safeReadPackageFromDir as safeReadPkgFromDir,
} from '@pnpm/utils'
import * as dp from 'dependency-path'
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
import {
  StoreController,
} from 'package-store'
import path = require('path')
import {
  satisfiesPackageJson,
  Shrinkwrap,
  ShrinkwrapImporter,
  write as saveShrinkwrap,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
  writeWantedOnly as saveWantedShrinkwrapOnly,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import semver = require('semver')
import {
  LAYOUT_VERSION,
  SHRINKWRAP_MINOR_VERSION,
} from '../constants'
import depsFromPackage, { getPreferredVersionsFromPackage } from '../depsFromPackage'
import depsToSpecs, { similarDepsToSpecs } from '../depsToSpecs'
import { PnpmError } from '../errorTypes'
import { absolutePathToRef } from '../fs/shrinkwrap'
import getSpecFromPackageJson from '../getSpecFromPackageJson'
import linkPackages, { DepGraphNodesByDepPath } from '../link'
import {
  createNodeId,
  nodeIdContainsSequence,
  ROOT_NODE_ID,
} from '../nodeIdUtils'
import parseWantedDependencies from '../parseWantedDependencies'
import resolveDependencies, { Pkg } from '../resolveDependencies'
import safeIsInnerLink from '../safeIsInnerLink'
import save from '../save'
import {
  WantedDependency,
} from '../types'
import extendOptions, {
  InstallOptions,
  StrictInstallOptions,
} from './extendInstallOptions'
import getContext, { PnpmContext } from './getContext'
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

export async function install (maybeOpts: InstallOptions & {
  preferredVersions?: {
    [packageName: string]: {
      selector: string,
      type: 'version' | 'range' | 'tag',
    },
  },
}) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const opts = await extendOptions(maybeOpts)

  if (!opts.include.dependencies && opts.include.optionalDependencies) {
    throw new PnpmError('ERR_PNPM_OPTIONAL_DEPS_REQUIRE_PROD_DEPS', 'Optional dependencies cannot be installed without production dependencies')
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
      !hasLocalTarballDepsInRoot(ctx.wantedShrinkwrap, ctx.importerPath) &&
      satisfiesPackageJson(ctx.wantedShrinkwrap, ctx.pkg, ctx.importerPath) &&
      await linkedPackagesSatisfyPackageJson(ctx.pkg, ctx.wantedShrinkwrap.importers[ctx.importerPath], opts.prefix, opts.localPackages))
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
          importerPath: ctx.importerPath,
          packageJson: ctx.pkg,
          shrinkwrapDirectory: ctx.shrinkwrapDirectory,
          wantedShrinkwrap: ctx.wantedShrinkwrap,
        } as HeadlessOptions)
        return
      }
    }

    const preferredVersions = maybeOpts.preferredVersions || getPreferredVersionsFromPackage(ctx.pkg)
    const specs = specsToInstallFromPackage(ctx.pkg)

    if (ctx.wantedShrinkwrap && ctx.wantedShrinkwrap.importers) {
      forgetResolutionsOfOldSpecs(ctx.wantedShrinkwrap.importers[ctx.importerPath], specs)
    }

    const scripts = !opts.ignoreScripts && ctx.pkg && ctx.pkg.scripts || {}
    if (opts.ignoreScripts && ctx.pkg && ctx.pkg.scripts &&
      (ctx.pkg.scripts.preinstall || ctx.pkg.scripts.prepublish ||
        ctx.pkg.scripts.install ||
        ctx.pkg.scripts.postinstall ||
        ctx.pkg.scripts.prepare)
    ) {
      ctx.pendingBuilds.push(ctx.importerPath)
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

// If the specifier is new, the old resolution probably does not satisfy it anymore.
// By removing these resolutions we ensure that they are resolved again using the new specs.
function forgetResolutionsOfOldSpecs (importer: ShrinkwrapImporter, specs: WantedDependency[]) {
  if (!importer.specifiers) return
  importer.dependencies = importer.dependencies || {}
  importer.devDependencies = importer.devDependencies || {}
  importer.optionalDependencies = importer.optionalDependencies || {}
  for (const spec of specs) {
    if (spec.alias && importer.specifiers[spec.alias] !== spec.pref) {
      if (importer.dependencies[spec.alias] && !importer.dependencies[spec.alias].startsWith('link:')) {
        delete importer.dependencies[spec.alias]
      }
      delete importer.devDependencies[spec.alias]
      delete importer.optionalDependencies[spec.alias]
    }
  }
}

async function linkedPackagesSatisfyPackageJson (
  pkg: PackageJson,
  shrImporter: ShrinkwrapImporter,
  prefix: string,
  localPackages?: LocalPackages,
) {
  const localPackagesByDirectory = localPackages ? getLocalPackagesByDirectory(localPackages) : {}
  for (const depField of DEPENDENCIES_FIELDS) {
    const importerDeps = shrImporter[depField]
    const pkgDeps = pkg[depField]
    if (!importerDeps || !pkgDeps) continue
    const depNames = Object.keys(importerDeps)
    for (const depName of depNames) {
      if (!importerDeps[depName].startsWith('link:') || !pkgDeps[depName]) continue
      const dir = path.join(prefix, importerDeps[depName].substr(5))
      const linkedPkg = localPackagesByDirectory[dir] || await safeReadPkgFromDir(dir)
      if (!linkedPkg || !semver.satisfies(linkedPkg.version, pkgDeps[depName])) return false
    }
  }
  return true
}

function getLocalPackagesByDirectory (localPackages: LocalPackages) {
  const localPackagesByDirectory = {}
  Object.keys(localPackages || {}).forEach((pkgName) => {
    Object.keys(localPackages[pkgName] || {}).forEach((pkgVersion) => {
      localPackagesByDirectory[localPackages[pkgName][pkgVersion].directory] = localPackages[pkgName][pkgVersion].package
    })
  })
  return localPackagesByDirectory
}

function hasLocalTarballDepsInRoot (shr: Shrinkwrap, importerPath: string) {
  const importer = shr.importers && shr.importers[importerPath]
  if (!importer) return false
  return R.any(refIsLocalTarball, R.values(importer.dependencies || {}))
    || R.any(refIsLocalTarball, R.values(importer.devDependencies || {}))
    || R.any(refIsLocalTarball, R.values(importer.optionalDependencies || {}))
}

function refIsLocalTarball (ref: string) {
  return ref.startsWith('file:') && (ref.endsWith('.tgz') || ref.endsWith('.tar.gz') || ref.endsWith('.tar'))
}

function specsToInstallFromPackage (
  pkg: PackageJson,
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
  const shrinkwrapNodeModulesPath = ctx.shrinkwrapDirectory === ctx.prefix
    ? nodeModulesPath : await realNodeModulesDir(ctx.shrinkwrapDirectory)

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
    nodeModules: shrinkwrapNodeModulesPath,
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
  if (!ctx.wantedShrinkwrap.importers || !ctx.wantedShrinkwrap.importers[ctx.importerPath]) {
    ctx.wantedShrinkwrap.importers = ctx.wantedShrinkwrap.importers || {}
    ctx.wantedShrinkwrap.importers[ctx.importerPath] = {specifiers: {}}
  }
  const shrImporter = ctx.wantedShrinkwrap.importers[ctx.importerPath]
  const installOpts = {
    currentDepth: 0,
    hasManifestInShrinkwrap,
    keypath: [],
    localPackages: opts.localPackages,
    parentNodeId: ROOT_NODE_ID,
    readPackageHook: opts.hooks.readPackage,
    reinstallForFlatten: opts.reinstallForFlatten,
    resolvedDependencies: {
      ...shrImporter.dependencies,
      ...shrImporter.devDependencies,
      ...shrImporter.optionalDependencies,
    },
    shamefullyFlatten: opts.shamefullyFlatten,
    sideEffectsCache: opts.sideEffectsCache,
    update: opts.update,
  }
  let nonLinkedPkgs: WantedDependency[]
  let linkedPkgs: Array<WantedDependency & {alias: string}>
  if (installType === 'named') {
    nonLinkedPkgs = packagesToInstall
    linkedPkgs = []
  } else {
    nonLinkedPkgs = []
    linkedPkgs = []
    for (const wantedDependency of packagesToInstall) {
      if (!wantedDependency.alias || opts.localPackages && opts.localPackages[wantedDependency.alias]) {
        nonLinkedPkgs.push(wantedDependency)
        continue
      }
      const isInnerLink = await safeIsInnerLink(shrinkwrapNodeModulesPath, wantedDependency.alias, {
        hideAlienModules: opts.shrinkwrapOnly === false,
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
    ctx.wantedShrinkwrap.importers[ctx.importerPath] = addDirectDependenciesToShrinkwrap(newPkg, shrImporter, linkedPkgs, pkgsToSave, ctx.wantedShrinkwrap.registry)
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

  const externalShrinkwrap = ctx.shrinkwrapDirectory !== opts.prefix
  const result = await linkPackages(rootNodeIdsByAlias, installCtx.pkgGraph, {
    afterAllResolvedHook: opts.hooks && opts.hooks.afterAllResolved,
    baseNodeModules: nodeModulesPath,
    bin: opts.bin,
    currentShrinkwrap: ctx.currentShrinkwrap,
    dryRun: opts.shrinkwrapOnly,
    externalShrinkwrap,
    force: opts.force,
    hoistedAliases: ctx.hoistedAliases,
    importerPath: ctx.importerPath,
    include: opts.include,
    independentLeaves: opts.independentLeaves,
    makePartialCurrentShrinkwrap,
    outdatedPkgs: installCtx.outdatedPkgs,
    pkg: newPkg || ctx.pkg,
    prefix: ctx.prefix,
    reinstallForFlatten: Boolean(opts.reinstallForFlatten),
    shamefullyFlatten: opts.shamefullyFlatten,
    shrinkwrapDirectoryNodeModules: installCtx.nodeModules,
    sideEffectsCache: opts.sideEffectsCache,
    skipped: ctx.skipped,
    storeController: opts.storeController,
    strictPeerDependencies: opts.strictPeerDependencies,
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
    await saveWantedShrinkwrapOnly(ctx.shrinkwrapDirectory, result.wantedShrinkwrap)
  } else {
    await Promise.all([
      opts.shrinkwrap
        ? saveShrinkwrap(ctx.shrinkwrapDirectory, result.wantedShrinkwrap, result.currentShrinkwrap)
        : saveCurrentShrinkwrapOnly(ctx.shrinkwrapDirectory, result.currentShrinkwrap),
      (() => {
        if (result.currentShrinkwrap.packages === undefined && result.removedDepPaths.size === 0) {
          return Promise.resolve()
        }
        return writeModulesYaml(installCtx.nodeModules, nodeModulesPath, {
          hoistedAliases: ctx.hoistedAliases,
          included: ctx.include,
          independentLeaves: opts.independentLeaves,
          layoutVersion: LAYOUT_VERSION,
          packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
          pendingBuilds: ctx.pendingBuilds,
          shamefullyFlatten: opts.shamefullyFlatten,
          skipped: Array.from(installCtx.skipped),
          store: ctx.storePath,
        })
      })(),
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
      const externalPkgs = installCtx.localPackages.map((localPackage) => ({
        alias: localPackage.alias,
        path: resolvePath(opts.prefix, localPackage.resolution.directory),
      }))
      await externalLink(externalPkgs, nodeModulesPath, linkOpts)
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

function addDirectDependenciesToShrinkwrap (
  newPkg: PackageJson,
  shrinkwrapImporter: ShrinkwrapImporter,
  linkedPkgs: Array<WantedDependency & {alias: string}>,
  pkgsToSave: Array<{
    alias: string,
    optional: boolean,
    dev: boolean,
    resolution: Resolution,
    id: string,
    version: string,
    name: string,
    specRaw: string,
    normalizedPref?: string,
  }>,
  standardRegistry: string,
): ShrinkwrapImporter {
  const newShrImporter = {
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
    specifiers: {},
  }

  linkedPkgs.forEach((linkedPkg) => {
    newShrImporter.specifiers[linkedPkg.alias] = getSpecFromPackageJson(newPkg as PackageJson, linkedPkg.alias) as string
  })
  if (shrinkwrapImporter.dependencies) {
    for (const alias of R.keys(shrinkwrapImporter.dependencies)) {
      if (shrinkwrapImporter.dependencies[alias].startsWith('link:')) {
        newShrImporter.dependencies[alias] = shrinkwrapImporter.dependencies[alias]
      }
    }
  }

  const pkgsToSaveByAlias = pkgsToSave.reduce((acc, pkgToSave) => {
    acc[pkgToSave.alias] = pkgToSave
    return acc
  }, {})

  const optionalDependencies = R.keys(newPkg.optionalDependencies)
  const dependencies = R.difference(R.keys(newPkg.dependencies), optionalDependencies)
  const devDependencies = R.difference(R.difference(R.keys(newPkg.devDependencies), optionalDependencies), dependencies)
  const allDeps = R.reduce(R.union, [], [optionalDependencies, devDependencies, dependencies]) as string[]

  for (const alias of allDeps) {
    if (pkgsToSaveByAlias[alias]) {
      const dep = pkgsToSaveByAlias[alias]
      const ref = absolutePathToRef(dep.id, {
        alias: dep.alias,
        realName: dep.name,
        resolution: dep.resolution,
        standardRegistry,
      })
      if (dep.dev) {
        newShrImporter.devDependencies[dep.alias] = ref
      } else if (dep.optional) {
        newShrImporter.optionalDependencies[dep.alias] = ref
      } else {
        newShrImporter.dependencies[dep.alias] = ref
      }
      newShrImporter.specifiers[dep.alias] = getSpecFromPackageJson(newPkg, dep.alias) as string
    } else if (typeof shrinkwrapImporter.specifiers[alias] !== 'undefined') {
      newShrImporter.specifiers[alias] = shrinkwrapImporter.specifiers[alias]
      if (shrinkwrapImporter.dependencies && shrinkwrapImporter.dependencies[alias]) {
        newShrImporter.dependencies[alias] = shrinkwrapImporter.dependencies[alias]
      } else if (shrinkwrapImporter.optionalDependencies && shrinkwrapImporter.optionalDependencies[alias]) {
        newShrImporter.optionalDependencies[alias] = shrinkwrapImporter.optionalDependencies[alias]
      } else if (shrinkwrapImporter.devDependencies && shrinkwrapImporter.devDependencies[alias]) {
        newShrImporter.devDependencies[alias] = shrinkwrapImporter.devDependencies[alias]
      }
    }
  }

  alignDependencyTypes(newPkg, newShrImporter)

  return newShrImporter
}

function alignDependencyTypes (pkg: PackageJson, shrImporter: ShrinkwrapImporter) {
  const depTypesOfAliases = getAliasToDependencyTypeMap(pkg)

  // Aligning the dependency types in shrinkwrap.yaml
  for (const depType of DEPENDENCIES_FIELDS) {
    if (!shrImporter[depType]) continue
    for (const alias of Object.keys(shrImporter[depType] || {})) {
      if (depType === depTypesOfAliases[alias] || !depTypesOfAliases[alias]) continue
      shrImporter[depTypesOfAliases[alias]][alias] = shrImporter[depType]![alias]
      delete shrImporter[depType]![alias]
    }
  }
}

function getAliasToDependencyTypeMap (pkg: PackageJson) {
  const depTypesOfAliases = {}
  for (const depType of DEPENDENCIES_FIELDS) {
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
