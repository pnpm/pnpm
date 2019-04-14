import buildModules, { linkBinsOfDependencies } from '@pnpm/build-modules'
import {
  LAYOUT_VERSION,
  LOCKFILE_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  packageJsonLogger,
  stageLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import headless from '@pnpm/headless'
import {
  runLifecycleHooksConcurrently,
} from '@pnpm/lifecycle'
import linkBins from '@pnpm/link-bins'
import {
  Lockfile,
  LockfileImporter,
  writeCurrentLockfile,
  writeLockfiles,
  writeWantedLockfile,
} from '@pnpm/lockfile-file'
import { satisfiesPackageJson } from '@pnpm/lockfile-utils'
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
  DEPENDENCIES_FIELDS,
  DependenciesField,
  DependencyPackageJson,
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
import isInnerLink = require('is-inner-link')
import isSubdir = require('is-subdir')
import pEvery from 'p-every'
import pFilter = require('p-filter')
import pLimit = require('p-limit')
import path = require('path')
import R = require('ramda')
import rimraf = require('rimraf-then')
import semver = require('semver')
import writePkg = require('write-pkg')
import { PnpmError } from '../errorTypes'
import getContext, { ImportersOptions, PnpmContext } from '../getContext'
import getSpecFromPackageJson from '../getSpecFromPackageJson'
import lock from '../lock'
import lockfilesEqual from '../lockfilesEqual'
import parseWantedDependencies from '../parseWantedDependencies'
import safeIsInnerLink from '../safeIsInnerLink'
import save from '../save'
import removeDeps from '../uninstall/removeDeps'
import getPref from '../utils/getPref'
import extendOptions, {
  InstallOptions,
  StrictInstallOptions,
} from './extendInstallOptions'
import linkPackages, {
  DependenciesGraph,
  DependenciesGraphNode,
  Importer as ImporterToLink,
} from './link'
import { absolutePathToRef } from './lockfile'

export type DependenciesMutation = (
  {
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
) & (
  {
    pkg: PackageJson,
  }
)

export async function install (
  pkg: PackageJson,
  opts: InstallOptions & {
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
  },
) {
  const importers = await mutateModules(
    [
      {
        buildIndex: 0,
        mutation: 'install',
        pkg,
        prefix: opts.prefix || process.cwd(),
      },
    ],
    opts,
  )
  return importers[0].pkg
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

  let result!: Array<{ prefix: string, pkg: PackageJson }>
  if (opts.lock) {
    result = await lock(ctx.lockfileDirectory, _install, {
      locks: opts.locks,
      prefix: ctx.lockfileDirectory,
      stale: opts.lockStaleDuration,
      storeController: opts.storeController,
    })
  } else {
    result = await _install()
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return result

  async function _install (): Promise<Array<{ prefix: string, pkg: PackageJson }>> {
    const installsOnly = importers.every((importer) => importer.mutation === 'install')
    if (
      !opts.lockfileOnly &&
      !opts.update &&
      installsOnly &&
      (
        opts.frozenLockfile ||
        opts.preferFrozenLockfile &&
        (!opts.pruneLockfileImporters || Object.keys(ctx.wantedLockfile.importers).length === ctx.importers.length) &&
        ctx.existsWantedLockfile &&
        ctx.wantedLockfile.lockfileVersion === LOCKFILE_VERSION &&
        await pEvery(ctx.importers, async (importer) =>
          !hasLocalTarballDepsInRoot(ctx.wantedLockfile, importer.id) &&
          satisfiesPackageJson(ctx.wantedLockfile, importer.pkg, importer.id) &&
          linkedPackagesAreUpToDate(importer.pkg, ctx.wantedLockfile.importers[importer.id], importer.prefix, opts.localPackages)
        )
      )
    ) {
      if (!ctx.existsWantedLockfile) {
        if (ctx.importers.some((importer) => pkgHasDependencies(importer.pkg))) {
          throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
        }
      } else {
        logger.info({ message: 'Performing headless installation', prefix: opts.lockfileDirectory })
        await headless({
          currentEngine: {
            nodeVersion: opts.nodeVersion,
            pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
          },
          currentLockfile: ctx.currentLockfile,
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
          lockfileDirectory: ctx.lockfileDirectory,
          ownLifecycleHooksStdio: opts.ownLifecycleHooksStdio,
          packageManager:  opts.packageManager,
          pendingBuilds: ctx.pendingBuilds,
          pruneStore: opts.pruneStore,
          rawNpmConfig: opts.rawNpmConfig,
          registries: opts.registries,
          sideEffectsCacheRead: opts.sideEffectsCacheRead,
          sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
          skipped: ctx.skipped,
          store: opts.store,
          storeController: opts.storeController,
          unsafePerm: opts.unsafePerm,
          userAgent: opts.userAgent,
          wantedLockfile: ctx.wantedLockfile,
        })
        return importers
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
            usesExternalLockfile: ctx.lockfileDirectory !== importer.prefix,
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
            usesExternalLockfile: ctx.lockfileDirectory !== importer.prefix,
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
          if (!packagesToInstall.length) return importers

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
          if (!packagesToInstall.length) return importers

          // TODO: install only those that were unlinked
          // but don't update their version specs in package.json
          await installCase({ ...importer, mutation: 'install' })
          break
        }
      }
    }

    async function installCase (importer: any) { // tslint:disable-line:no-any
      const wantedDeps = getWantedDependencies(importer.pkg)

      if (ctx.wantedLockfile && ctx.wantedLockfile.importers) {
        forgetResolutionsOfPrevWantedDeps(ctx.wantedLockfile.importers[importer.id], wantedDeps)
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
          lockfileOnly: opts.lockfileOnly,
          modulesDir: importer.modulesDir,
          prefix: importer.prefix,
          storePath: ctx.storePath,
          virtualStoreDir: ctx.virtualStoreDir,
        }),
        newPkgRawSpecs: [],
        updatePackageJson: false,
        usesExternalLockfile: ctx.lockfileDirectory !== importer.prefix,
        wantedDeps,
      })
    }

    // Unfortunately, the private lockfile may differ from the public one.
    // A user might run named installations on a project that has a pnpm-lock.yaml file before running a noop install
    const makePartialCurrentLockfile = !installsOnly && (
      ctx.existsWantedLockfile && !ctx.existsCurrentLockfile ||
      // TODO: this operation is quite expensive. We'll have to find a better solution to do this.
      // maybe in pnpm v2 it won't be needed. See: https://github.com/pnpm/pnpm/issues/841
      !lockfilesEqual(ctx.currentLockfile, ctx.wantedLockfile)
    )
    const result = await installInContext(importersToInstall, ctx, {
      ...opts,
      makePartialCurrentLockfile,
      update: opts.update || !installsOnly,
      updateLockfileMinorVersion: true,
    })

    if (!opts.ignoreScripts) {
      await runLifecycleHooksConcurrently(['install', 'postinstall', 'prepublish', 'prepare'],
        importersToBeInstalled,
        opts.childConcurrency,
        scriptsOpts,
      )
    }

    return result
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
    lockfileOnly: boolean,
    prefix: string,
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
      hideAlienModules: opts.lockfileOnly === false,
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
function forgetResolutionsOfPrevWantedDeps (importer: LockfileImporter, wantedDeps: WantedDependency[]) {
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
  lockfileImporter: LockfileImporter,
  prefix: string,
  localPackages?: LocalPackages,
) {
  const localPackagesByDirectory = localPackages ? getLocalPackagesByDirectory(localPackages) : {}
  for (const depField of DEPENDENCIES_FIELDS) {
    const importerDeps = lockfileImporter[depField]
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

function hasLocalTarballDepsInRoot (lockfile: Lockfile, importerId: string) {
  const importer = lockfile.importers && lockfile.importers[importerId]
  if (!importer) return false
  return R.any(refIsLocalTarball, R.values(importer.dependencies || {}))
    || R.any(refIsLocalTarball, R.values(importer.devDependencies || {}))
    || R.any(refIsLocalTarball, R.values(importer.optionalDependencies || {}))
}

function refIsLocalTarball (ref: string) {
  return ref.startsWith('file:') && (ref.endsWith('.tgz') || ref.endsWith('.tar.gz') || ref.endsWith('.tar'))
}

export async function addDependenciesToPackage (
  pkg: PackageJson,
  dependencySelectors: string[],
  opts: InstallOptions & {
    allowNew?: boolean,
    prefix?: string,
    pinnedVersion?: 'major' | 'minor' | 'patch',
    targetDependenciesField?: DependenciesField,
  },
) {
  const importers = await mutateModules(
    [
      {
        allowNew: opts.allowNew,
        dependencySelectors,
        mutation: 'installSome',
        pinnedVersion: opts.pinnedVersion,
        pkg,
        prefix: opts.prefix || process.cwd(),
        shamefullyFlatten: opts.shamefullyFlatten,
        targetDependenciesField: opts.targetDependenciesField,
      },
    ],
    {
      ...opts,
      lockfileDirectory: opts.lockfileDirectory || opts.prefix,
    })
  return importers[0].pkg
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
  usesExternalLockfile: boolean,
  wantedDeps: WantedDependency[],
} & DependenciesMutation

async function installInContext (
  importers: ImporterToUpdate[],
  ctx: PnpmContext<DependenciesMutation>,
  opts: StrictInstallOptions & {
    makePartialCurrentLockfile: boolean,
    updateLockfileMinorVersion: boolean,
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
  },
) {
  if (opts.lockfileOnly && ctx.existsCurrentLockfile) {
    logger.warn({
      message: '`node_modules` is present. Lockfile only installation will make it out-of-date',
      prefix: ctx.lockfileDirectory,
    })
  }

  // Avoid requesting package meta info from registry only when the lockfile version is at least the expected
  const hasManifestInLockfile = ctx.wantedLockfile.lockfileVersion >= LOCKFILE_VERSION

  ctx.wantedLockfile.importers = ctx.wantedLockfile.importers || {}
  for (const importer of importers) {
    if (!ctx.wantedLockfile.importers[importer.id]) {
      ctx.wantedLockfile.importers[importer.id] = { specifiers: {} }
    }
  }
  if (opts.pruneLockfileImporters) {
    const importerIds = new Set(importers.map((importer) => importer.id))
    for (const wantedImporter of Object.keys(ctx.wantedLockfile.importers)) {
      if (!importerIds.has(wantedImporter)) {
        delete ctx.wantedLockfile.importers[wantedImporter]
      }
    }
  }

  await Promise.all(
    importers
      .map(async (importer) => {
        if (importer.mutation !== 'uninstallSome') return
        importer.pkg = await removeDeps(importer.pkg, importer.dependencyNames, {
          prefix: importer.prefix,
          saveType: importer.targetDependenciesField,
        })
      }),
  )

  stageLogger.debug({
    prefix: ctx.lockfileDirectory,
    stage: 'resolution_started',
  })

  const {
    dependenciesTree,
    outdatedDependencies,
    resolvedImporters,
    resolvedPackagesByPackageId,
    wantedToBeSkippedPackageIds,
  } = await resolveDependencies({
    currentLockfile: ctx.currentLockfile,
    dryRun: opts.lockfileOnly,
    engineStrict: opts.engineStrict,
    force: opts.force,
    hasManifestInLockfile,
    hooks: opts.hooks,
    importers,
    localPackages: opts.localPackages,
    lockfileDirectory: opts.lockfileDirectory,
    nodeVersion: opts.nodeVersion,
    pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
    preferredVersions: opts.preferredVersions,
    registries: opts.registries,
    resolutionStrategy: opts.resolutionStrategy,
    sideEffectsCache: opts.sideEffectsCacheRead,
    storeController: opts.storeController,
    tag: opts.tag,
    updateDepth: (() => {
      // This can be remove from lockfile v4
      if (!hasManifestInLockfile) {
        // The lockfile has to be updated to contain
        // the necessary info from package manifests
        return Infinity
      }
      if (opts.force) return Infinity
      if (opts.update) {
        return opts.depth
      }
      if (
        modulesIsUpToDate({
          currentLockfile: ctx.currentLockfile,
          defaultRegistry: ctx.registries.default,
          skippedRelDepPaths: Array.from(ctx.skipped),
          wantedLockfile: ctx.wantedLockfile,
        })
      ) {
        return opts.repeatInstallDepth
      }
      return Infinity
    })(),
    virtualStoreDir: ctx.virtualStoreDir,
    wantedLockfile: ctx.wantedLockfile,
  })

  stageLogger.debug({
    prefix: ctx.lockfileDirectory,
    stage: 'resolution_done',
  })

  const importersToLink = await Promise.all<ImporterToLink>(importers.map(async (importer) => {
    const resolvedImporter = resolvedImporters[importer.id]
    let newPkg: PackageJson | undefined = importer.pkg
    let writePackageJson!: boolean
    if (importer.updatePackageJson && importer.mutation === 'installSome') {
      if (!importer.pkg) {
        throw new Error('Cannot save because no package.json found')
      }
      writePackageJson = true
      const specsToUsert = <any>resolvedImporter.directDependencies // tslint:disable-line
        .filter((dep) => importer.newPkgRawSpecs.includes(dep.specRaw))
        .map((dep) => {
          return {
            name: dep.alias,
            pref: dep.normalizedPref || getPref(dep.alias, dep.name, dep.version, {
              pinnedVersion: importer.pinnedVersion,
              rawSpec: dep.specRaw,
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
        importer.pkg,
        specsToUsert,
        { dryRun: true },
      )
    } else {
      writePackageJson = false
      packageJsonLogger.debug({
        prefix: importer.prefix,
        updated: importer.pkg,
      })
    }

    if (newPkg) {
      const lockfileImporter = ctx.wantedLockfile.importers[importer.id]
      ctx.wantedLockfile.importers[importer.id] = addDirectDependenciesToLockfile(
        newPkg,
        lockfileImporter,
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
              .filter((directDep) => importer.newPkgRawSpecs.includes(directDep.specRaw))
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
      usesExternalLockfile: importer.usesExternalLockfile,
      writePackageJson,
    }
  }))

  const result = await linkPackages(
    importersToLink,
    dependenciesTree,
    {
      afterAllResolvedHook: opts.hooks && opts.hooks.afterAllResolved,
      currentLockfile: ctx.currentLockfile,
      dryRun: opts.lockfileOnly,
      force: opts.force,
      include: opts.include,
      independentLeaves: opts.independentLeaves,
      lockfileDirectory: opts.lockfileDirectory,
      makePartialCurrentLockfile: opts.makePartialCurrentLockfile,
      outdatedDependencies,
      pruneStore: opts.pruneStore,
      registries: ctx.registries,
      skipped: ctx.skipped,
      storeController: opts.storeController,
      strictPeerDependencies: opts.strictPeerDependencies,
      updateLockfileMinorVersion: opts.updateLockfileMinorVersion,
      virtualStoreDir: ctx.virtualStoreDir,
      wantedLockfile: ctx.wantedLockfile,
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

  if (!opts.lockfileOnly) {
    // postinstall hooks
    if (!opts.ignoreScripts && result.newDepPaths && result.newDepPaths.length) {
      const depPaths = Object.keys(result.depGraph)
      const rootNodes = depPaths.filter((depPath) => result.depGraph[depPath].depth === 0)

      await buildModules(result.depGraph, rootNodes, {
        childConcurrency: opts.childConcurrency,
        depsToBuild: new Set(result.newDepPaths),
        optional: opts.include.optionalDependencies,
        prefix: ctx.lockfileDirectory,
        rawNpmConfig: opts.rawNpmConfig,
        rootNodeModulesDir: ctx.virtualStoreDir,
        sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
        storeController: opts.storeController,
        unsafePerm: opts.unsafePerm,
        userAgent: opts.userAgent,
      })
    }

    if (result.newDepPaths && result.newDepPaths.length) {
      const newPkgs = R.props<string, DependenciesGraphNode>(result.newDepPaths, result.depGraph)
      await linkAllBins(newPkgs, result.depGraph, {
        optional: opts.include.optionalDependencies,
        warn: (message: string) => logger.warn({ message, prefix: opts.lockfileDirectory }),
      })
    }

    if (!opts.lockfileOnly) {
      await Promise.all(importersToLink.map(linkBinsOfImporter))
    }
  }

  const lockfileOpts = { forceSharedFormat: opts.forceSharedLockfile }
  if (opts.lockfileOnly) {
    await writeWantedLockfile(ctx.lockfileDirectory, result.wantedLockfile, lockfileOpts)
  } else {
    await Promise.all([
      opts.useLockfile
        ? writeLockfiles(ctx.lockfileDirectory, result.wantedLockfile, result.currentLockfile, lockfileOpts)
        : writeCurrentLockfile(ctx.lockfileDirectory, result.currentLockfile, lockfileOpts),
      (() => {
        if (result.currentLockfile.packages === undefined && result.removedDepPaths.size === 0) {
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

  summaryLogger.debug({ prefix: opts.lockfileDirectory })

  await opts.storeController.close()

  return importersToLink.map((importer) => ({ prefix: importer.prefix, pkg: importer.pkg }))
}

const limitLinking = pLimit(16)

function linkBinsOfImporter ({ modulesDir, bin, prefix }: ImporterToLink) {
  const warn = (message: string) => logger.warn({ message, prefix })
  return linkBins(modulesDir, bin, { warn })
}

async function linkAllBins (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    optional: boolean,
    warn: (message: string) => void,
  },
) {
  return Promise.all(
    depNodes.map((depNode => limitLinking(async () => linkBinsOfDependencies(depNode, depGraph, opts)))),
  )
}

function modulesIsUpToDate (
  opts: {
    defaultRegistry: string,
    currentLockfile: Lockfile,
    wantedLockfile: Lockfile,
    skippedRelDepPaths: string[],
  }
) {
  const currentWithSkipped = [
    ...R.keys(opts.currentLockfile.packages),
    ...opts.skippedRelDepPaths,
  ]
  currentWithSkipped.sort()
  return R.equals(R.keys(opts.wantedLockfile.packages), currentWithSkipped)
}

function addDirectDependenciesToLockfile (
  newPkg: PackageJson,
  lockfileImporter: LockfileImporter,
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
): LockfileImporter {
  const newLockfileImporter = {
    dependencies: {},
    devDependencies: {},
    optionalDependencies: {},
    specifiers: {},
  }

  linkedPackages.forEach((linkedPkg) => {
    newLockfileImporter.specifiers[linkedPkg.alias] = getSpecFromPackageJson(newPkg, linkedPkg.alias)
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
        newLockfileImporter.devDependencies[dep.alias] = ref
      } else if (dep.optional) {
        newLockfileImporter.optionalDependencies[dep.alias] = ref
      } else {
        newLockfileImporter.dependencies[dep.alias] = ref
      }
      newLockfileImporter.specifiers[dep.alias] = getSpecFromPackageJson(newPkg, dep.alias)
    } else if (lockfileImporter.specifiers[alias]) {
      newLockfileImporter.specifiers[alias] = lockfileImporter.specifiers[alias]
      if (lockfileImporter.dependencies && lockfileImporter.dependencies[alias]) {
        newLockfileImporter.dependencies[alias] = lockfileImporter.dependencies[alias]
      } else if (lockfileImporter.optionalDependencies && lockfileImporter.optionalDependencies[alias]) {
        newLockfileImporter.optionalDependencies[alias] = lockfileImporter.optionalDependencies[alias]
      } else if (lockfileImporter.devDependencies && lockfileImporter.devDependencies[alias]) {
        newLockfileImporter.devDependencies[alias] = lockfileImporter.devDependencies[alias]
      }
    }
  }

  alignDependencyTypes(newPkg, newLockfileImporter)

  return newLockfileImporter
}

function alignDependencyTypes (pkg: PackageJson, lockfileImporter: LockfileImporter) {
  const depTypesOfAliases = getAliasToDependencyTypeMap(pkg)

  // Aligning the dependency types in pnpm-lock.yaml
  for (const depType of DEPENDENCIES_FIELDS) {
    if (!lockfileImporter[depType]) continue
    for (const alias of Object.keys(lockfileImporter[depType] || {})) {
      if (depType === depTypesOfAliases[alias] || !depTypesOfAliases[alias]) continue
      lockfileImporter[depTypesOfAliases[alias]][alias] = lockfileImporter[depType]![alias]
      delete lockfileImporter[depType]![alias]
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
  return pkgs.filter(Boolean).map((pkg: DependencyPackageJson) => ({
    name: pkg.name,
    version: pkg.version,
  }))
}
