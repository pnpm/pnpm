import {
  packageJsonLogger,
  skippedOptionalDependencyLogger,
  stageLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import headless from '@pnpm/headless'
import {
  runLifecycleHooksConcurrently,
  runPostinstallHooks,
} from '@pnpm/lifecycle'
import logger, {
  streamParser,
} from '@pnpm/logger'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import readModulesDirs from '@pnpm/read-modules-dir'
import resolveDependencies, { ResolvedPackage } from '@pnpm/resolve-dependencies'
import {
  LocalPackages,
  Resolution,
} from '@pnpm/resolver-base'
import {
  Shrinkwrap,
  ShrinkwrapImporter,
  write as saveShrinkwrap,
  writeCurrentOnly as saveCurrentShrinkwrapOnly,
  writeWantedOnly as saveWantedShrinkwrapOnly,
} from '@pnpm/shrinkwrap-file'
import { satisfiesPackageJson } from '@pnpm/shrinkwrap-utils'
import {
  DEPENDENCIES_FIELDS,
  DependenciesField,
  PackageJson,
  Registries,
} from '@pnpm/types'
import {
  getAllDependenciesFromPackage,
  getWantedDependencies,
  safeReadPackageFromDir as safeReadPkgFromDir,
  WantedDependency,
} from '@pnpm/utils'
import * as dp from 'dependency-path'
import graphSequencer = require('graph-sequencer')
import isInnerLink = require('is-inner-link')
import isSubdir = require('is-subdir')
import pEvery = require('p-every')
import pFilter = require('p-filter')
import path = require('path')
import R = require('ramda')
import rimraf = require('rimraf-then')
import runGroups from 'run-groups'
import semver = require('semver')
import {
  ENGINE_NAME,
  LAYOUT_VERSION,
  SHRINKWRAP_VERSION,
} from '../constants'
import { PnpmError } from '../errorTypes'
import getContext, { ImportersOptions, PnpmContext } from '../getContext'
import getSpecFromPackageJson from '../getSpecFromPackageJson'
import lock from '../lock'
import parseWantedDependencies from '../parseWantedDependencies'
import safeIsInnerLink from '../safeIsInnerLink'
import save from '../save'
import shrinkwrapsEqual from '../shrinkwrapsEqual'
import removeDeps from '../uninstall/removeDeps'
import getPref from '../utils/getPref'
import extendOptions, {
  InstallOptions,
  StrictInstallOptions,
} from './extendInstallOptions'
import linkPackages, {
  DependenciesGraph,
  Importer as ImporterToLink,
} from './link'
import { absolutePathToRef } from './shrinkwrap'

export type DependenciesMutation = {
  buildIndex: number,
  mutation: 'install',
  pruneDirectDependencies?: boolean,
} | {
  allowNew?: boolean,
  dependencySelectors: string[],
  mutation: 'installSome',
  pruneDirectDependencies?: boolean,
  pinnedVersion?: 'major' | 'minor' | 'patch',
  targetDependenciesField?: DependenciesField,
} | {
  mutation: 'uninstallSome',
  dependencyNames: string[],
  targetDependenciesField?: DependenciesField,
} | {
  mutation: 'unlink',
} | {
  mutation: 'unlinkSome',
  dependencyNames: string[],
}

export function install (
  opts: InstallOptions & {
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
  },
) {
  return mutateModules(
    [
      {
        buildIndex: 0,
        mutation: 'install',
        prefix: opts.prefix || process.cwd(),
      },
    ],
    opts,
  )
}

export type MutatedImporter = ImportersOptions & DependenciesMutation

export async function mutateModules (
  importers: MutatedImporter[],
  maybeOpts: InstallOptions & {
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
  },
) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const opts = await extendOptions(maybeOpts)

  if (!opts.include.dependencies && opts.include.optionalDependencies) {
    throw new PnpmError('ERR_PNPM_OPTIONAL_DEPS_REQUIRE_PROD_DEPS', 'Optional dependencies cannot be installed without production dependencies')
  }

  const ctx = await getContext(importers, opts)

  for (const importer of ctx.importers) {
    if (!importer.pkg) {
      throw new Error(`No package.json found in "${importer.prefix}"`)
    }
  }

  if (opts.lock) {
    await lock(ctx.shrinkwrapDirectory, _install, {
      locks: opts.locks,
      prefix: ctx.shrinkwrapDirectory,
      stale: opts.lockStaleDuration,
      storeController: opts.storeController,
    })
  } else {
    await _install()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  async function _install () {
    const installsOnly = importers.every((importer) => importer.mutation === 'install')
    if (
      !opts.shrinkwrapOnly &&
      !opts.update &&
      installsOnly &&
      (
        opts.frozenShrinkwrap ||
        opts.preferFrozenShrinkwrap &&
        (!opts.pruneShrinkwrapImporters || Object.keys(ctx.wantedShrinkwrap.importers).length === ctx.importers.length) &&
        ctx.existsWantedShrinkwrap &&
        ctx.wantedShrinkwrap.shrinkwrapVersion === SHRINKWRAP_VERSION &&
        await pEvery(ctx.importers, async (importer) =>
          !hasLocalTarballDepsInRoot(ctx.wantedShrinkwrap, importer.id) &&
          satisfiesPackageJson(ctx.wantedShrinkwrap, importer.pkg, importer.id) &&
          linkedPackagesAreUpToDate(importer.pkg, ctx.wantedShrinkwrap.importers[importer.id], importer.prefix, opts.localPackages)
        )
      )
    ) {
      if (!ctx.existsWantedShrinkwrap) {
        if (ctx.importers.some((importer) => pkgHasDependencies(importer.pkg))) {
          throw new Error('Headless installation requires a shrinkwrap.yaml file')
        }
      } else {
        logger.info({ message: 'Performing headless installation', prefix: opts.shrinkwrapDirectory })
        await headless({
          currentEngine: {
            nodeVersion: opts.nodeVersion,
            pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
          },
          currentShrinkwrap: ctx.currentShrinkwrap,
          engineStrict: opts.engineStrict,
          force: opts.force,
          ignoreScripts: opts.ignoreScripts,
          importers: ctx.importers as Array<{
            bin: string,
            buildIndex: number,
            hoistedAliases: {[depPath: string]: string[]}
            id: string,
            modulesDir: string,
            pkg: PackageJson,
            prefix: string,
            pruneDirectDependencies?: boolean,
            shamefullyFlatten: boolean,
          }>,
          include: opts.include,
          independentLeaves: opts.independentLeaves,
          ownLifecycleHooksStdio: opts.ownLifecycleHooksStdio,
          packageManager:  opts.packageManager,
          pendingBuilds: ctx.pendingBuilds,
          pruneStore: opts.pruneStore,
          rawNpmConfig: opts.rawNpmConfig,
          registries: opts.registries,
          shrinkwrapDirectory: ctx.shrinkwrapDirectory,
          sideEffectsCacheRead: opts.sideEffectsCacheRead,
          sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
          skipped: ctx.skipped,
          store: opts.store,
          storeController: opts.storeController,
          unsafePerm: opts.unsafePerm,
          userAgent: opts.userAgent,
          verifyStoreIntegrity: opts.verifyStoreIntegrity,
          wantedShrinkwrap: ctx.wantedShrinkwrap,
        })
        return
      }
    }

    const importersToInstall = [] as ImporterToUpdate[]

    const importersToBeInstalled = ctx.importers.filter((importer) => importer.mutation === 'install') as Array<{ buildIndex: number, prefix: string, pkg: PackageJson, modulesDir: string }>
    const scriptsOpts = {
      rawNpmConfig: opts.rawNpmConfig,
      stdio: opts.ownLifecycleHooksStdio,
      unsafePerm: opts.unsafePerm || false,
    }
    if (!opts.ignoreScripts) {
      await runLifecycleHooksConcurrently(
        ['preinstall'],
        importersToBeInstalled,
        opts.childConcurrency,
        scriptsOpts,
      )
    }

    // TODO: make it concurrent
    for (const importer of ctx.importers) {
      switch (importer.mutation) {
        case 'uninstallSome':
          importersToInstall.push({
            pruneDirectDependencies: false,
            ...importer,
            linkedPackages: [],
            newPkgRawSpecs: [],
            nonLinkedPackages: [],
            removePackages: importer.dependencyNames,
            updatePackageJson: true,
            usesExternalShrinkwrap: ctx.shrinkwrapDirectory !== importer.prefix,
            wantedDeps: [],
          })
          break
        case 'install': {
          await installCase(importer)
          break
        }
        case 'installSome': {
          const currentPrefs = opts.ignoreCurrentPrefs ? {} : getAllDependenciesFromPackage(importer.pkg)
          const optionalDependencies = importer.targetDependenciesField ? {} : importer.pkg.optionalDependencies || {}
          const devDependencies = importer.targetDependenciesField ? {} : importer.pkg.devDependencies || {}
          const wantedDeps = parseWantedDependencies(importer.dependencySelectors, {
            allowNew: importer.allowNew !== false,
            currentPrefs,
            defaultTag: opts.tag,
            dev: importer.targetDependenciesField === 'devDependencies',
            devDependencies,
            optional: importer.targetDependenciesField === 'optionalDependencies',
            optionalDependencies,
          })
          importersToInstall.push({
            pruneDirectDependencies: false,
            ...importer,
            linkedPackages: [],
            newPkgRawSpecs: wantedDeps.map((wantedDependency) => wantedDependency.raw),
            nonLinkedPackages: wantedDeps,
            updatePackageJson: true,
            usesExternalShrinkwrap: ctx.shrinkwrapDirectory !== importer.prefix,
            wantedDeps,
          })
          break
        }
        case 'unlink': {
          const packageDirs = await readModulesDirs(importer.modulesDir)
          const externalPackages = await pFilter(
            packageDirs,
            (packageDir: string) => isExternalLink(ctx.storePath, importer.modulesDir, packageDir),
          )
          const allDeps = getAllDependenciesFromPackage(importer.pkg)
          const packagesToInstall: string[] = []
          for (const pkgName of externalPackages) {
            await rimraf(path.join(importer.modulesDir, pkgName))
            if (allDeps[pkgName]) {
              packagesToInstall.push(pkgName)
            }
          }
          if (!packagesToInstall.length) return

          // TODO: install only those that were unlinked
          // but don't update their version specs in package.json
          await installCase({ ...importer, mutation: 'install' })
          break
        }
        case 'unlinkSome': {
          const packagesToInstall: string[] = []
          const allDeps = getAllDependenciesFromPackage(importer.pkg)
          for (const depName of importer.dependencyNames) {
            try {
              if (!await isExternalLink(ctx.storePath, importer.modulesDir, depName)) {
                logger.warn({
                  message: `${depName} is not an external link`,
                  prefix: importer.prefix,
                })
                continue
              }
            } catch (err) {
              if (err['code'] !== 'ENOENT') throw err // tslint:disable-line:no-string-literal
            }
            await rimraf(path.join(importer.modulesDir, depName))
            if (allDeps[depName]) {
              packagesToInstall.push(depName)
            }
          }
          if (!packagesToInstall.length) return

          // TODO: install only those that were unlinked
          // but don't update their version specs in package.json
          await installCase({ ...importer, mutation: 'install' })
          break
        }
      }
    }

    async function installCase (importer: any) { // tslint:disable-line:no-any
      const wantedDeps = getWantedDependencies(importer.pkg)

      if (ctx.wantedShrinkwrap && ctx.wantedShrinkwrap.importers) {
        forgetResolutionsOfPrevWantedDeps(ctx.wantedShrinkwrap.importers[importer.id], wantedDeps)
      }
      const scripts = !opts.ignoreScripts && importer.pkg && importer.pkg.scripts || {}
      if (opts.ignoreScripts && importer.pkg && importer.pkg.scripts &&
        (importer.pkg.scripts.preinstall || importer.pkg.scripts.prepublish ||
          importer.pkg.scripts.install ||
          importer.pkg.scripts.postinstall ||
          importer.pkg.scripts.prepare)
      ) {
        ctx.pendingBuilds.push(importer.id)
      }

      if (scripts['prepublish']) { // tslint:disable-line:no-string-literal
        logger.warn({
          message: '`prepublish` scripts are deprecated. Use `prepare` for build steps and `prepublishOnly` for upload-only.',
          prefix: importer.prefix,
        })
      }

      importersToInstall.push({
        pruneDirectDependencies: false,
        ...importer,
        ...await partitionLinkedPackages(wantedDeps, {
          localPackages: opts.localPackages,
          modulesDir: importer.modulesDir,
          prefix: importer.prefix,
          shrinkwrapOnly: opts.shrinkwrapOnly,
          storePath: ctx.storePath,
          virtualStoreDir: ctx.virtualStoreDir,
        }),
        newPkgRawSpecs: [],
        updatePackageJson: false,
        usesExternalShrinkwrap: ctx.shrinkwrapDirectory !== importer.prefix,
        wantedDeps,
      })
    }

    // Unfortunately, the private shrinkwrap file may differ from the public one.
    // A user might run named installations on a project that has a shrinkwrap.yaml file before running a noop install
    const makePartialCurrentShrinkwrap = !installsOnly && (
      ctx.existsWantedShrinkwrap && !ctx.existsCurrentShrinkwrap ||
      // TODO: this operation is quite expensive. We'll have to find a better solution to do this.
      // maybe in pnpm v2 it won't be needed. See: https://github.com/pnpm/pnpm/issues/841
      !shrinkwrapsEqual(ctx.currentShrinkwrap, ctx.wantedShrinkwrap)
    )
    await installInContext(importersToInstall, ctx, {
      ...opts,
      makePartialCurrentShrinkwrap,
      update: opts.update || !installsOnly,
      updateShrinkwrapMinorVersion: true,
    })

    if (!opts.ignoreScripts) {
      await runLifecycleHooksConcurrently(['install', 'postinstall', 'prepublish', 'prepare'],
        importersToBeInstalled,
        opts.childConcurrency,
        scriptsOpts,
      )
    }
  }
}

async function isExternalLink (store: string, modules: string, pkgName: string) {
  const link = await isInnerLink(modules, pkgName)

  // checking whether the link is pointing to the store is needed
  // because packages are linked to store when independent-leaves = true
  return !link.isInner && !isSubdir(store, link.target)
}

function pkgHasDependencies (pkg: PackageJson) {
  return Boolean(
    R.keys(pkg.dependencies).length ||
    R.keys(pkg.devDependencies).length ||
    R.keys(pkg.optionalDependencies).length
  )
}

async function partitionLinkedPackages (
  wantedDeps: WantedDependency[],
  opts: {
    modulesDir: string,
    localPackages?: LocalPackages,
    prefix: string,
    shrinkwrapOnly: boolean,
    storePath: string,
    virtualStoreDir: string,
  },
) {
  const nonLinkedPackages: WantedDependency[] = []
  const linkedPackages: Array<WantedDependency & {alias: string}> = []
  for (const wantedDependency of wantedDeps) {
    if (!wantedDependency.alias || opts.localPackages && opts.localPackages[wantedDependency.alias]) {
      nonLinkedPackages.push(wantedDependency)
      continue
    }
    const isInnerLink = await safeIsInnerLink(opts.virtualStoreDir, wantedDependency.alias, {
      hideAlienModules: opts.shrinkwrapOnly === false,
      prefix: opts.prefix,
      storePath: opts.storePath,
    })
    if (isInnerLink === true) {
      nonLinkedPackages.push(wantedDependency)
      continue
    }
    // This info-log might be better to be moved to the reporter
    logger.info({
      message: `${wantedDependency.alias} is linked to ${opts.modulesDir} from ${isInnerLink}`,
      prefix: opts.prefix,
    })
    linkedPackages.push(wantedDependency as (WantedDependency & {alias: string}))
  }
  return {
    linkedPackages,
    nonLinkedPackages,
  }
}

// If the specifier is new, the old resolution probably does not satisfy it anymore.
// By removing these resolutions we ensure that they are resolved again using the new specs.
function forgetResolutionsOfPrevWantedDeps (importer: ShrinkwrapImporter, wantedDeps: WantedDependency[]) {
  if (!importer.specifiers) return
  importer.dependencies = importer.dependencies || {}
  importer.devDependencies = importer.devDependencies || {}
  importer.optionalDependencies = importer.optionalDependencies || {}
  for (const wantedDep of wantedDeps) {
    if (wantedDep.alias && importer.specifiers[wantedDep.alias] !== wantedDep.pref) {
      if (importer.dependencies[wantedDep.alias] && !importer.dependencies[wantedDep.alias].startsWith('link:')) {
        delete importer.dependencies[wantedDep.alias]
      }
      delete importer.devDependencies[wantedDep.alias]
      delete importer.optionalDependencies[wantedDep.alias]
    }
  }
}

async function linkedPackagesAreUpToDate (
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
      if (!pkgDeps[depName]) continue
      const isLinked = importerDeps[depName].startsWith('link:')
      if (isLinked && (pkgDeps[depName].startsWith('link:') || pkgDeps[depName].startsWith('file:'))) continue
      const dir = isLinked
        ? path.join(prefix, importerDeps[depName].substr(5))
        : (localPackages && localPackages[depName] && localPackages[depName] && localPackages[depName][importerDeps[depName]] && localPackages[depName][importerDeps[depName]].directory)
      if (!dir) continue
      const linkedPkg = localPackagesByDirectory[dir] || await safeReadPkgFromDir(dir)
      const localPackageSatisfiesRange = linkedPkg && semver.satisfies(linkedPkg.version, pkgDeps[depName])
      if (isLinked !== localPackageSatisfiesRange) return false
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

function hasLocalTarballDepsInRoot (shr: Shrinkwrap, importerId: string) {
  const importer = shr.importers && shr.importers[importerId]
  if (!importer) return false
  return R.any(refIsLocalTarball, R.values(importer.dependencies || {}))
    || R.any(refIsLocalTarball, R.values(importer.devDependencies || {}))
    || R.any(refIsLocalTarball, R.values(importer.optionalDependencies || {}))
}

function refIsLocalTarball (ref: string) {
  return ref.startsWith('file:') && (ref.endsWith('.tgz') || ref.endsWith('.tar.gz') || ref.endsWith('.tar'))
}

export async function addDependenciesToPackage (
  dependencySelectors: string[],
  opts: InstallOptions & {
    allowNew?: boolean,
    prefix?: string,
    pinnedVersion?: 'major' | 'minor' | 'patch',
    targetDependenciesField?: DependenciesField,
  },
) {
  return mutateModules(
    [
      {
        allowNew: opts.allowNew,
        dependencySelectors,
        mutation: 'installSome',
        pinnedVersion: opts.pinnedVersion,
        prefix: opts.prefix || process.cwd(),
        shamefullyFlatten: opts.shamefullyFlatten,
        targetDependenciesField: opts.targetDependenciesField,
      },
    ],
    {
      ...opts,
      shrinkwrapDirectory: opts.shrinkwrapDirectory || opts.prefix,
    })
}

type ImporterToUpdate = {
  bin: string,
  hoistedAliases: {[depPath: string]: string[]},
  id: string,
  linkedPackages: Array<WantedDependency & {alias: string}>,
  modulesDir: string,
  newPkgRawSpecs: string[],
  nonLinkedPackages: WantedDependency[],
  pkg: PackageJson,
  prefix: string,
  pruneDirectDependencies: boolean,
  removePackages?: string[],
  shamefullyFlatten: boolean,
  updatePackageJson: boolean,
  usesExternalShrinkwrap: boolean,
  wantedDeps: WantedDependency[],
} & DependenciesMutation

async function installInContext (
  importers: ImporterToUpdate[],
  ctx: PnpmContext<DependenciesMutation>,
  opts: StrictInstallOptions & {
    makePartialCurrentShrinkwrap: boolean,
    updateShrinkwrapMinorVersion: boolean,
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
  },
) {
  if (opts.shrinkwrapOnly && ctx.existsCurrentShrinkwrap) {
    logger.warn({
      message: '`node_modules` is present. Shrinkwrap only installation will make it out-of-date',
      prefix: ctx.shrinkwrapDirectory,
    })
  }

  // Avoid requesting package meta info from registry only when the shrinkwrap version is at least the expected
  const hasManifestInShrinkwrap = ctx.wantedShrinkwrap.shrinkwrapVersion >= SHRINKWRAP_VERSION

  ctx.wantedShrinkwrap.importers = ctx.wantedShrinkwrap.importers || {}
  for (const importer of importers) {
    if (!ctx.wantedShrinkwrap.importers[importer.id]) {
      ctx.wantedShrinkwrap.importers[importer.id] = { specifiers: {} }
    }
  }
  if (opts.pruneShrinkwrapImporters) {
    const importerIds = new Set(importers.map((importer) => importer.id))
    for (const wantedImporter of Object.keys(ctx.wantedShrinkwrap.importers)) {
      if (!importerIds.has(wantedImporter)) {
        delete ctx.wantedShrinkwrap.importers[wantedImporter]
      }
    }
  }

  await Promise.all(
    importers
      .map(async (importer) => {
        if (importer.mutation !== 'uninstallSome') return
        const pkgJsonPath = path.join(importer.prefix, 'package.json')
        importer.pkg = await removeDeps(pkgJsonPath, importer.dependencyNames, {
          prefix: importer.prefix,
          saveType: importer.targetDependenciesField,
        })
      }),
  )

  stageLogger.debug({
    prefix: ctx.shrinkwrapDirectory,
    stage: 'resolution_started',
  })

  const {
    dependenciesTree,
    outdatedDependencies,
    resolvedImporters,
    resolvedPackagesByPackageId,
    wantedToBeSkippedPackageIds,
  } = await resolveDependencies({
    currentShrinkwrap: ctx.currentShrinkwrap,
    dryRun: opts.shrinkwrapOnly,
    engineStrict: opts.engineStrict,
    force: opts.force,
    hasManifestInShrinkwrap,
    hooks: opts.hooks,
    importers,
    localPackages: opts.localPackages,
    nodeVersion: opts.nodeVersion,
    pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
    preferredVersions: opts.preferredVersions,
    registries: opts.registries,
    shrinkwrapDirectory: opts.shrinkwrapDirectory,
    sideEffectsCache: opts.sideEffectsCacheRead,
    storeController: opts.storeController,
    tag: opts.tag,
    updateDepth: (() => {
      // This can be remove from shrinkwrap v4
      if (!hasManifestInShrinkwrap) {
        // The shrinkwrap file has to be updated to contain
        // the necessary info from package manifests
        return Infinity
      }
      if (opts.update) {
        return opts.depth
      }
      if (
        modulesIsUpToDate({
          currentShrinkwrap: ctx.currentShrinkwrap,
          defaultRegistry: ctx.registries.default,
          skippedRelDepPaths: Array.from(ctx.skipped),
          wantedShrinkwrap: ctx.wantedShrinkwrap,
        })
      ) {
        return opts.repeatInstallDepth
      }
      return Infinity
    })(),
    verifyStoreIntegrity: opts.verifyStoreIntegrity,
    virtualStoreDir: ctx.virtualStoreDir,
    wantedShrinkwrap: ctx.wantedShrinkwrap,
  })

  stageLogger.debug({
    prefix: ctx.shrinkwrapDirectory,
    stage: 'resolution_done',
  })

  const importersToLink = await Promise.all<ImporterToLink>(importers.map(async (importer) => {
    const resolvedImporter = resolvedImporters[importer.id]
    let newPkg: PackageJson | undefined = importer.pkg
    if (importer.updatePackageJson && importer.mutation === 'installSome') {
      if (!importer.pkg) {
        throw new Error('Cannot save because no package.json found')
      }
      const specsToUsert = <any>resolvedImporter.directDependencies // tslint:disable-line
        .filter((dep) => importer.newPkgRawSpecs.indexOf(dep.specRaw) !== -1)
        .map((dep) => {
          return {
            name: dep.alias,
            pref: dep.normalizedPref || getPref(dep.alias, dep.name, dep.version, {
              pinnedVersion: importer.pinnedVersion,
            }),
            saveType: importer.targetDependenciesField,
          }
        })
      for (const pkgToInstall of importer.wantedDeps) {
        if (pkgToInstall.alias && !specsToUsert.some((spec: any) => spec.name === pkgToInstall.alias)) { // tslint:disable-line
          specsToUsert.push({
            name: pkgToInstall.alias,
            saveType: importer.targetDependenciesField,
          })
        }
      }
      newPkg = await save(
        importer.prefix,
        specsToUsert,
      )
    } else {
      packageJsonLogger.debug({
        prefix: importer.prefix,
        updated: importer.pkg,
      })
    }

    if (newPkg) {
      const shrImporter = ctx.wantedShrinkwrap.importers[importer.id]
      ctx.wantedShrinkwrap.importers[importer.id] = addDirectDependenciesToShrinkwrap(
        newPkg,
        shrImporter,
        importer.linkedPackages,
        resolvedImporter.directDependencies,
        ctx.registries,
      )
    }

    const topParents = importer.pkg
      ? await getTopParents(
          R.difference(
            R.keys(getAllDependenciesFromPackage(importer.pkg)),
            importer.newPkgRawSpecs && resolvedImporter.directDependencies
              .filter((directDep) => importer.newPkgRawSpecs.indexOf(directDep.specRaw) !== -1)
              .map((directDep) => directDep.alias) || [],
          ),
          importer.modulesDir,
        )
      : []

    return {
      bin: importer.bin,
      directNodeIdsByAlias: resolvedImporter.directNodeIdsByAlias,
      hoistedAliases: importer.hoistedAliases,
      id: importer.id,
      linkedDependencies: resolvedImporter.linkedDependencies,
      modulesDir: importer.modulesDir,
      pkg: newPkg || importer.pkg,
      prefix: importer.prefix,
      pruneDirectDependencies: importer.pruneDirectDependencies,
      removePackages: importer.removePackages,
      shamefullyFlatten: importer.shamefullyFlatten,
      topParents,
      usesExternalShrinkwrap: importer.usesExternalShrinkwrap,
    }
  }))

  const result = await linkPackages(
    importersToLink,
    dependenciesTree,
    {
      afterAllResolvedHook: opts.hooks && opts.hooks.afterAllResolved,
      currentShrinkwrap: ctx.currentShrinkwrap,
      dryRun: opts.shrinkwrapOnly,
      force: opts.force,
      include: opts.include,
      independentLeaves: opts.independentLeaves,
      makePartialCurrentShrinkwrap: opts.makePartialCurrentShrinkwrap,
      outdatedDependencies,
      pruneStore: opts.pruneStore,
      registries: ctx.registries,
      shrinkwrapDirectory: opts.shrinkwrapDirectory,
      skipped: ctx.skipped,
      storeController: opts.storeController,
      strictPeerDependencies: opts.strictPeerDependencies,
      updateShrinkwrapMinorVersion: opts.updateShrinkwrapMinorVersion,
      virtualStoreDir: ctx.virtualStoreDir,
      wantedShrinkwrap: ctx.wantedShrinkwrap,
      wantedToBeSkippedPackageIds,
    },
  )

  ctx.pendingBuilds = ctx.pendingBuilds
    .filter((relDepPath) => !result.removedDepPaths.has(dp.resolve(ctx.registries, relDepPath)))

  if (opts.ignoreScripts) {
    // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
    ctx.pendingBuilds = ctx.pendingBuilds
      .concat(
        result.newDepPaths
          .filter((depPath) => result.depGraph[depPath].requiresBuild)
          .map((depPath) => dp.relative(ctx.registries, result.depGraph[depPath].name, depPath)),
      )
  }

  const shrinkwrapOpts = { forceSharedFormat: opts.forceSharedShrinkwrap }
  if (opts.shrinkwrapOnly) {
    await saveWantedShrinkwrapOnly(ctx.shrinkwrapDirectory, result.wantedShrinkwrap, shrinkwrapOpts)
  } else {
    await Promise.all([
      opts.shrinkwrap
        ? saveShrinkwrap(ctx.shrinkwrapDirectory, result.wantedShrinkwrap, result.currentShrinkwrap, shrinkwrapOpts)
        : saveCurrentShrinkwrapOnly(ctx.shrinkwrapDirectory, result.currentShrinkwrap, shrinkwrapOpts),
      (() => {
        if (result.currentShrinkwrap.packages === undefined && result.removedDepPaths.size === 0) {
          return Promise.resolve()
        }
        return writeModulesYaml(ctx.virtualStoreDir, {
          ...ctx.modulesFile,
          importers: {
            ...ctx.modulesFile && ctx.modulesFile.importers,
            ...importersToLink.reduce((acc, importer) => {
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
      })(),
    ])

    // postinstall hooks
    if (!(opts.ignoreScripts || !result.newDepPaths || !result.newDepPaths.length)) {
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
      const groups = chunks.map((chunk) => chunk
        .filter((depPath) => result.depGraph[depPath].requiresBuild && !result.depGraph[depPath].isBuilt && result.newDepPaths.indexOf(depPath) !== -1)
        .map((depPath) => result.depGraph[depPath])
        .map((pkg) => async () => {
          try {
            const hasSideEffects = await runPostinstallHooks({
              depPath: pkg.absolutePath,
              optional: pkg.optional,
              pkgRoot: pkg.peripheralLocation,
              prepare: pkg.prepare,
              rawNpmConfig: opts.rawNpmConfig,
              rootNodeModulesDir: ctx.virtualStoreDir,
              unsafePerm: opts.unsafePerm || false,
            })
            if (hasSideEffects && opts.sideEffectsCacheWrite) {
              try {
                await opts.storeController.upload(pkg.peripheralLocation, {
                  engine: ENGINE_NAME,
                  pkgId: pkg.id,
                })
              } catch (err) {
                if (err && err.statusCode === 403) {
                  logger.warn({
                    message: `The store server disabled upload requests, could not upload ${pkg.id}`,
                    prefix: ctx.shrinkwrapDirectory,
                  })
                } else {
                  logger.warn({
                    error: err,
                    message: `An error occurred while uploading ${pkg.id}`,
                    prefix: ctx.shrinkwrapDirectory,
                  })
                }
              }
            }
          } catch (err) {
            if (resolvedPackagesByPackageId[pkg.id].optional) {
              // TODO: add parents field to the log
              skippedOptionalDependencyLogger.debug({
                details: err.toString(),
                package: {
                  id: pkg.id,
                  name: pkg.name,
                  version: pkg.version,
                },
                prefix: opts.shrinkwrapDirectory,
                reason: 'build_failure',
              })
              return
            }
            throw err
          }
        }),
      )
      await runGroups(opts.childConcurrency, groups)
    }
  }

  // waiting till the skipped packages are downloaded to the store
  await Promise.all(
    R.props<string, ResolvedPackage>(Array.from(ctx.skipped), resolvedPackagesByPackageId)
      // skipped packages might have not been reanalized on a repeat install
      // so lets just ignore those by excluding nulls
      .filter(Boolean)
      .map((pkg) => pkg.fetchingFiles),
  )

  // waiting till package requests are finished
  await Promise.all(R.values(resolvedPackagesByPackageId).map((installed) => installed.finishing))

  summaryLogger.debug({ prefix: opts.shrinkwrapDirectory })

  await opts.storeController.close()
}

function modulesIsUpToDate (
  opts: {
    defaultRegistry: string,
    currentShrinkwrap: Shrinkwrap,
    wantedShrinkwrap: Shrinkwrap,
    skippedRelDepPaths: string[],
  }
) {
  const currentWithSkipped = [
    ...R.keys(opts.currentShrinkwrap.packages),
    ...opts.skippedRelDepPaths,
  ]
  currentWithSkipped.sort()
  return R.equals(R.keys(opts.wantedShrinkwrap.packages), currentWithSkipped)
}

function getSubgraphToBuild (
  graph: DependenciesGraph,
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
  linkedPackages: Array<WantedDependency & {alias: string}>,
  directDependencies: Array<{
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
  registries: Registries,
): ShrinkwrapImporter {
  const newShrImporter = {
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
    specifiers: {},
  }

  linkedPackages.forEach((linkedPkg) => {
    newShrImporter.specifiers[linkedPkg.alias] = getSpecFromPackageJson(newPkg, linkedPkg.alias)
  })

  const directDependenciesByAlias = directDependencies.reduce((acc, directDependency) => {
    acc[directDependency.alias] = directDependency
    return acc
  }, {})

  const optionalDependencies = R.keys(newPkg.optionalDependencies)
  const dependencies = R.difference(R.keys(newPkg.dependencies), optionalDependencies)
  const devDependencies = R.difference(R.difference(R.keys(newPkg.devDependencies), optionalDependencies), dependencies)
  const allDeps = R.reduce(R.union, [], [optionalDependencies, devDependencies, dependencies]) as string[]

  for (const alias of allDeps) {
    if (directDependenciesByAlias[alias]) {
      const dep = directDependenciesByAlias[alias]
      const ref = absolutePathToRef(dep.id, {
        alias: dep.alias,
        realName: dep.name,
        registries,
        resolution: dep.resolution,
      })
      if (dep.dev) {
        newShrImporter.devDependencies[dep.alias] = ref
      } else if (dep.optional) {
        newShrImporter.optionalDependencies[dep.alias] = ref
      } else {
        newShrImporter.dependencies[dep.alias] = ref
      }
      newShrImporter.specifiers[dep.alias] = getSpecFromPackageJson(newPkg, dep.alias)
    } else if (shrinkwrapImporter.specifiers[alias]) {
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

async function getTopParents (pkgNames: string[], modules: string) {
  const pkgs = await Promise.all(
    pkgNames.map((pkgName) => path.join(modules, pkgName)).map(safeReadPkgFromDir),
  )
  return pkgs.filter(Boolean).map((pkg: PackageJson) => ({
    name: pkg.name,
    version: pkg.version,
  }))
}
