import buildModules, { linkBinsOfDependencies } from '@pnpm/build-modules'
import {
  LAYOUT_VERSION,
  LOCKFILE_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  packageManifestLogger,
  stageLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import PnpmError from '@pnpm/error'
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
import { satisfiesPackageManifest } from '@pnpm/lockfile-utils'
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
  DependencyManifest,
  ImporterManifest,
  Registries,
} from '@pnpm/types'
import {
  getAllDependenciesFromPackage,
  getWantedDependencies,
  safeReadPackageFromDir as safeReadPkgFromDir,
  WantedDependency,
} from '@pnpm/utils'
import rimraf = require('@zkochan/rimraf')
import * as dp from 'dependency-path'
import isInnerLink = require('is-inner-link')
import isSubdir = require('is-subdir')
import pEvery from 'p-every'
import pFilter = require('p-filter')
import pLimit from 'p-limit'
import path = require('path')
import R = require('ramda')
import semver = require('semver')
import getContext, { ImportersOptions, PnpmContext } from '../getContext'
import getSpecFromPackageManifest from '../getSpecFromPackageManifest'
import lock from '../lock'
import lockfilesEqual from '../lockfilesEqual'
import parseWantedDependencies from '../parseWantedDependencies'
import safeIsInnerLink from '../safeIsInnerLink'
import save, { PackageSpecObject } from '../save'
import removeDeps from '../uninstall/removeDeps'
import getPref from '../utils/getPref'
import extendOptions, {
  InstallOptions,
  StrictInstallOptions,
} from './extendInstallOptions'
import getPreferredVersionsFromPackage from './getPreferredVersions'
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
    peer?: boolean,
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
    manifest: ImporterManifest,
  }
)

export async function install (
  manifest: ImporterManifest,
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
        manifest,
        mutation: 'install',
        prefix: opts.prefix || process.cwd(),
      },
    ],
    opts,
  )
  return importers[0].manifest
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
    throw new PnpmError('OPTIONAL_DEPS_REQUIRE_PROD_DEPS', 'Optional dependencies cannot be installed without production dependencies')
  }

  const ctx = await getContext(importers, opts)

  for (const { manifest, prefix } of ctx.importers) {
    if (!manifest) {
      throw new Error(`No package.json found in "${prefix}"`)
    }
  }

  let result!: Array<{ prefix: string, manifest: ImporterManifest }>
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

  async function _install (): Promise<Array<{ prefix: string, manifest: ImporterManifest }>> {
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
          satisfiesPackageManifest(ctx.wantedLockfile, importer.manifest, importer.id) &&
          linkedPackagesAreUpToDate(importer.manifest, ctx.wantedLockfile.importers[importer.id], importer.prefix, opts.localPackages)
        )
      )
    ) {
      if (!ctx.existsWantedLockfile) {
        if (ctx.importers.some((importer) => pkgHasDependencies(importer.manifest))) {
          throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
        }
      } else {
        logger.info({ message: 'Lockfile is up-to-date, resolution step is skipped', prefix: opts.lockfileDirectory })
        await headless({
          currentEngine: {
            nodeVersion: opts.nodeVersion,
            pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
          },
          currentLockfile: ctx.currentLockfile,
          engineStrict: opts.engineStrict,
          extraBinPaths: opts.extraBinPaths,
          force: opts.force,
          hoistedAliases: ctx.hoistedAliases,
          hoistPattern: ctx.hoistPattern,
          ignoreScripts: opts.ignoreScripts,
          importers: ctx.importers as Array<{
            bin: string,
            buildIndex: number,
            id: string,
            manifest: ImporterManifest,
            modulesDir: string,
            prefix: string,
            pruneDirectDependencies?: boolean,
          }>,
          include: opts.include,
          independentLeaves: opts.independentLeaves,
          lockfileDirectory: ctx.lockfileDirectory,
          ownLifecycleHooksStdio: opts.ownLifecycleHooksStdio,
          packageManager:  opts.packageManager,
          pendingBuilds: ctx.pendingBuilds,
          pruneStore: opts.pruneStore,
          rawConfig: opts.rawConfig,
          registries: opts.registries,
          shamefullyHoist: ctx.shamefullyHoist,
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

    const importersToBeInstalled = ctx.importers.filter(({ mutation }) => mutation === 'install') as Array<{ buildIndex: number, prefix: string, manifest: ImporterManifest, modulesDir: string }>
    const scriptsOpts = {
      extraBinPaths: opts.extraBinPaths,
      rawConfig: opts.rawConfig,
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
            newPkgRawSpecs: [],
            removePackages: importer.dependencyNames,
            updatePackageManifest: true,
            wantedDeps: [],
          })
          break
        case 'install': {
          await installCase(importer)
          break
        }
        case 'installSome': {
          await installSome(importer)
          break
        }
        case 'unlink': {
          const packageDirs = await readModulesDirs(importer.modulesDir)
          const externalPackages = await pFilter(
            packageDirs,
            (packageDir: string) => isExternalLink(ctx.storePath, importer.modulesDir, packageDir),
          )
          const allDeps = getAllDependenciesFromPackage(importer.manifest)
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
          const allDeps = getAllDependenciesFromPackage(importer.manifest)
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
          await installSome({ ...importer, mutation: 'installSome', dependencySelectors: packagesToInstall }, false)
          break
        }
      }
    }

    async function installCase (importer: any) { // tslint:disable-line:no-any
      const wantedDeps = getWantedDependencies(importer.manifest)

      if (ctx.wantedLockfile && ctx.wantedLockfile.importers) {
        forgetResolutionsOfPrevWantedDeps(ctx.wantedLockfile.importers[importer.id], wantedDeps)
      }
      const scripts = !opts.ignoreScripts && importer.manifest && importer.manifest.scripts || {}
      if (opts.ignoreScripts && importer.manifest && importer.manifest.scripts &&
        (importer.manifest.scripts.preinstall || importer.manifest.scripts.prepublish ||
          importer.manifest.scripts.install ||
          importer.manifest.scripts.postinstall ||
          importer.manifest.scripts.prepare)
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
        newPkgRawSpecs: [],
        updatePackageManifest: false,
        wantedDeps,
      })
    }

    async function installSome (importer: any, updatePackageManifest: boolean = true) { // tslint:disable-line:no-any
      const currentPrefs = opts.ignoreCurrentPrefs ? {} : getAllDependenciesFromPackage(importer.manifest)
      const optionalDependencies = importer.targetDependenciesField ? {} : importer.manifest.optionalDependencies || {}
      const devDependencies = importer.targetDependenciesField ? {} : importer.manifest.devDependencies || {}
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
        newPkgRawSpecs: wantedDeps.map(({ raw }) => raw),
        updatePackageManifest,
        wantedDeps,
      })
    }

    const equalLockfiles = lockfilesEqual(ctx.currentLockfile, ctx.wantedLockfile)
    const currentLockfileIsUpToDate = !ctx.existsWantedLockfile || equalLockfiles
    // Unfortunately, the private lockfile may differ from the public one.
    // A user might run named installations on a project that has a pnpm-lock.yaml file before running a noop install
    const makePartialCurrentLockfile = !installsOnly && (
      ctx.existsWantedLockfile && !ctx.existsCurrentLockfile ||
      // TODO: this operation is quite expensive. We'll have to find a better solution to do this.
      // maybe in pnpm v2 it won't be needed. See: https://github.com/pnpm/pnpm/issues/841
      !equalLockfiles
    )
    const result = await installInContext(importersToInstall, ctx, {
      ...opts,
      currentLockfileIsUpToDate,
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

function pkgHasDependencies (manifest: ImporterManifest) {
  return Boolean(
    R.keys(manifest.dependencies).length ||
    R.keys(manifest.devDependencies).length ||
    R.keys(manifest.optionalDependencies).length
  )
}

async function partitionLinkedPackages (
  dependencies: WantedDependency[],
  opts: {
    modulesDir: string,
    localPackages?: LocalPackages,
    lockfileOnly: boolean,
    prefix: string,
    storePath: string,
    virtualStoreDir: string,
  },
) {
  const nonLinkedDependencies: WantedDependency[] = []
  const linkedAliases = new Set<string>()
  for (const dependency of dependencies) {
    if (!dependency.alias || opts.localPackages && opts.localPackages[dependency.alias]) {
      nonLinkedDependencies.push(dependency)
      continue
    }
    const isInnerLink = await safeIsInnerLink(opts.modulesDir, dependency.alias, {
      hideAlienModules: opts.lockfileOnly === false,
      prefix: opts.prefix,
      storePath: opts.storePath,
      virtualStoreDir: opts.virtualStoreDir,
    })
    if (isInnerLink === true) {
      nonLinkedDependencies.push(dependency)
      continue
    }
    // This info-log might be better to be moved to the reporter
    logger.info({
      message: `${dependency.alias} is linked to ${opts.modulesDir} from ${isInnerLink}`,
      prefix: opts.prefix,
    })
    linkedAliases.add(dependency.alias)
  }
  return {
    linkedAliases,
    nonLinkedDependencies,
  }
}

// If the specifier is new, the old resolution probably does not satisfy it anymore.
// By removing these resolutions we ensure that they are resolved again using the new specs.
function forgetResolutionsOfPrevWantedDeps (importer: LockfileImporter, wantedDeps: WantedDependency[]) {
  if (!importer.specifiers) return
  importer.dependencies = importer.dependencies || {}
  importer.devDependencies = importer.devDependencies || {}
  importer.optionalDependencies = importer.optionalDependencies || {}
  for (const { alias, pref } of wantedDeps) {
    if (alias && importer.specifiers[alias] !== pref) {
      if (importer.dependencies[alias] && !importer.dependencies[alias].startsWith('link:')) {
        delete importer.dependencies[alias]
      }
      delete importer.devDependencies[alias]
      delete importer.optionalDependencies[alias]
    }
  }
}

async function linkedPackagesAreUpToDate (
  manifest: ImporterManifest,
  lockfileImporter: LockfileImporter,
  prefix: string,
  localPackages?: LocalPackages,
) {
  const localPackagesByDirectory = localPackages ? getLocalPackagesByDirectory(localPackages) : {}
  for (const depField of DEPENDENCIES_FIELDS) {
    const importerDeps = lockfileImporter[depField]
    const pkgDeps = manifest[depField]
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
      const availableVersion = pkgDeps[depName].startsWith('workspace:') ? pkgDeps[depName].substr(10) : pkgDeps[depName]
      const localPackageSatisfiesRange = linkedPkg && semver.satisfies(linkedPkg.version, availableVersion)
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
  manifest: ImporterManifest,
  dependencySelectors: string[],
  opts: InstallOptions & {
    allowNew?: boolean,
    peer?: boolean,
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
        manifest,
        mutation: 'installSome',
        peer: opts.peer,
        pinnedVersion: opts.pinnedVersion,
        prefix: opts.prefix || process.cwd(),
        targetDependenciesField: opts.targetDependenciesField,
      },
    ],
    {
      ...opts,
      lockfileDirectory: opts.lockfileDirectory || opts.prefix,
    })
  return importers[0].manifest
}

type ImporterToUpdate = {
  bin: string,
  id: string,
  manifest: ImporterManifest,
  modulesDir: string,
  newPkgRawSpecs: string[],
  prefix: string,
  pruneDirectDependencies: boolean,
  removePackages?: string[],
  updatePackageManifest: boolean,
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
    currentLockfileIsUpToDate: boolean,
  },
) {
  if (opts.lockfileOnly && ctx.existsCurrentLockfile) {
    logger.warn({
      message: '`node_modules` is present. Lockfile only installation will make it out-of-date',
      prefix: ctx.lockfileDirectory,
    })
  }

  ctx.wantedLockfile.importers = ctx.wantedLockfile.importers || {}
  for (const { id } of importers) {
    if (!ctx.wantedLockfile.importers[id]) {
      ctx.wantedLockfile.importers[id] = { specifiers: {} }
    }
  }
  if (opts.pruneLockfileImporters) {
    const importerIds = new Set(importers.map(({ id }) => id))
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
        importer.manifest = await removeDeps(importer.manifest, importer.dependencyNames, {
          prefix: importer.prefix,
          saveType: importer.targetDependenciesField,
        })
      }),
  )

  stageLogger.debug({
    prefix: ctx.lockfileDirectory,
    stage: 'resolution_started',
  })

  const defaultUpdateDepth = (() => {
    if (opts.force) return Infinity
    if (opts.update) {
      return opts.depth
    }
    return -1
  })()
  const _toResolveImporter = toResolveImporter.bind(null, {
    defaultUpdateDepth,
    localPackages: opts.localPackages,
    lockfileOnly: opts.lockfileOnly,
    preferredVersions: opts.preferredVersions,
    storePath: ctx.storePath,
    virtualStoreDir: ctx.virtualStoreDir,
  })
  const {
    dependenciesTree,
    outdatedDependencies,
    resolvedImporters,
    resolvedPackagesByPackageId,
    wantedToBeSkippedPackageIds,
  } = await resolveDependencies(
    await Promise.all(importers.map((importer) => _toResolveImporter(importer, Boolean(ctx.hoistPattern && importer.id === '.')))),
    {
      currentLockfile: ctx.currentLockfile,
      dryRun: opts.lockfileOnly,
      engineStrict: opts.engineStrict,
      force: opts.force,
      hooks: opts.hooks,
      localPackages: opts.localPackages,
      lockfileDirectory: opts.lockfileDirectory,
      nodeVersion: opts.nodeVersion,
      pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
      registries: opts.registries,
      resolutionStrategy: opts.resolutionStrategy,
      sideEffectsCache: opts.sideEffectsCacheRead,
      storeController: opts.storeController,
      tag: opts.tag,
      updateLockfile: ctx.wantedLockfile.lockfileVersion !== LOCKFILE_VERSION || !opts.currentLockfileIsUpToDate,
      virtualStoreDir: ctx.virtualStoreDir,
      wantedLockfile: ctx.wantedLockfile,
    },
  )

  stageLogger.debug({
    prefix: ctx.lockfileDirectory,
    stage: 'resolution_done',
  })

  const importersToLink = await Promise.all<ImporterToLink>(importers.map(async (importer) => {
    const resolvedImporter = resolvedImporters[importer.id]
    let newPkg: ImporterManifest | undefined = importer.manifest
    if (importer.updatePackageManifest && importer.mutation === 'installSome') {
      if (!importer.manifest) {
        throw new Error('Cannot save because no package.json found')
      }
      const specsToUpsert: PackageSpecObject[] = resolvedImporter.directDependencies
        .filter(({ specRaw }) => importer.newPkgRawSpecs.includes(specRaw))
        .map(({ alias, name, normalizedPref, specRaw, version, resolution }) => {
          let pref!: string
          if (normalizedPref) {
            pref = normalizedPref
          } else {
            pref = getPref(alias, name, version, {
              pinnedVersion: importer.pinnedVersion,
              rawSpec: specRaw,
            })
            if (resolution.type === 'directory' && opts.saveWorkspaceProtocol) {
              pref = `workspace:${pref}`
            }
          }
          return {
            name: alias,
            peer: importer.peer,
            pref,
            saveType: importer.targetDependenciesField,
          }
        })
      for (const pkgToInstall of importer.wantedDeps) {
        if (pkgToInstall.alias && !specsToUpsert.some(({ name }) => name === pkgToInstall.alias)) {
          specsToUpsert.push({
            name: pkgToInstall.alias,
            peer: importer.peer,
            saveType: importer.targetDependenciesField,
          })
        }
      }
      newPkg = await save(
        importer.prefix,
        importer.manifest,
        specsToUpsert,
        { dryRun: true },
      )
    } else {
      packageManifestLogger.debug({
        prefix: importer.prefix,
        updated: importer.manifest,
      })
    }

    if (newPkg) {
      const lockfileImporter = ctx.wantedLockfile.importers[importer.id]
      ctx.wantedLockfile.importers[importer.id] = addDirectDependenciesToLockfile(
        newPkg,
        lockfileImporter,
        resolvedImporter.linkedDependencies,
        resolvedImporter.directDependencies,
        ctx.registries,
      )
    }

    const topParents = importer.manifest
      ? await getTopParents(
          R.difference(
            Object.keys(getAllDependenciesFromPackage(importer.manifest)),
            importer.newPkgRawSpecs && resolvedImporter.directDependencies
              .filter(({ specRaw }) => importer.newPkgRawSpecs.includes(specRaw))
              .map(({ alias }) => alias) || [],
          ),
          importer.modulesDir,
        )
      : []

    return {
      bin: importer.bin,
      directNodeIdsByAlias: resolvedImporter.directNodeIdsByAlias,
      id: importer.id,
      linkedDependencies: resolvedImporter.linkedDependencies,
      manifest: newPkg || importer.manifest,
      modulesDir: importer.modulesDir,
      prefix: importer.prefix,
      pruneDirectDependencies: importer.pruneDirectDependencies,
      removePackages: importer.removePackages,
      topParents,
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
      hoistedAliases: ctx.hoistedAliases,
      hoistedModulesDir: ctx.hoistedModulesDir,
      hoistPattern: ctx.hoistPattern,
      include: opts.include,
      independentLeaves: opts.independentLeaves,
      lockfileDirectory: opts.lockfileDirectory,
      makePartialCurrentLockfile: opts.makePartialCurrentLockfile,
      outdatedDependencies,
      pruneStore: opts.pruneStore,
      registries: ctx.registries,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
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
        extraBinPaths: ctx.extraBinPaths,
        optional: opts.include.optionalDependencies,
        prefix: ctx.lockfileDirectory,
        rawConfig: opts.rawConfig,
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

  // waiting till the skipped packages are downloaded to the store
  await Promise.all(
    R.props<string, ResolvedPackage>(Array.from(ctx.skipped), resolvedPackagesByPackageId)
      // skipped packages might have not been reanalized on a repeat install
      // so lets just ignore those by excluding nulls
      .filter(Boolean)
      .map(({ fetchingFiles }) => fetchingFiles()),
  )

  // waiting till package requests are finished
  await Promise.all(R.values(resolvedPackagesByPackageId).map(({ finishing }) => finishing()))

  const lockfileOpts = { forceSharedFormat: opts.forceSharedLockfile }
  if (opts.lockfileOnly) {
    await writeWantedLockfile(ctx.lockfileDirectory, result.wantedLockfile, lockfileOpts)
  } else {
    await Promise.all([
      opts.useLockfile
        ? writeLockfiles({
          currentLockfile: result.currentLockfile,
          currentLockfileDir: ctx.virtualStoreDir,
          wantedLockfile: result.wantedLockfile,
          wantedLockfileDir: ctx.lockfileDirectory,
          ...lockfileOpts,
        })
        : writeCurrentLockfile(ctx.virtualStoreDir, result.currentLockfile, lockfileOpts),
      (() => {
        if (result.currentLockfile.packages === undefined && result.removedDepPaths.size === 0) {
          return Promise.resolve()
        }
        return writeModulesYaml(ctx.rootModulesDir, {
          ...ctx.modulesFile,
          hoistedAliases: result.newHoistedAliases,
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
      })(),
    ])
  }

  summaryLogger.debug({ prefix: opts.lockfileDirectory })

  await opts.storeController.close()

  return importersToLink.map(({ manifest, prefix }) => ({ prefix, manifest }))
}

async function toResolveImporter (
  opts: {
    defaultUpdateDepth: number,
    localPackages: LocalPackages,
    lockfileOnly: boolean,
    storePath: string,
    virtualStoreDir: string,
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
  },
  importer: ImporterToUpdate,
  hoist: boolean,
) {
  const allDeps = getWantedDependencies(importer.manifest)
  const { linkedAliases, nonLinkedDependencies } = await partitionLinkedPackages(allDeps, {
    localPackages: opts.localPackages,
    lockfileOnly: opts.lockfileOnly,
    modulesDir: importer.modulesDir,
    prefix: importer.prefix,
    storePath: opts.storePath,
    virtualStoreDir: opts.virtualStoreDir,
  })
  const depsToUpdate = importer.wantedDeps.map((wantedDep) => ({
    ...wantedDep,
    isNew: true,
  }))
  const existingDeps = nonLinkedDependencies
    .filter(({ alias }) => !importer.wantedDeps.some((wantedDep) => wantedDep.alias === alias))
  let wantedDependencies!: Array<WantedDependency & { updateDepth: number }>
  if (!importer.manifest || hoist) {
    wantedDependencies = [
      ...depsToUpdate,
      ...existingDeps,
    ]
    .map((dep) => ({
      ...dep,
      updateDepth: hoist ? Infinity : opts.defaultUpdateDepth,
    }))
  } else {
    wantedDependencies = [
      ...depsToUpdate.map((dep) => ({ ...dep, updateDepth: opts.defaultUpdateDepth })),
      ...existingDeps.map((dep) => ({ ...dep, updateDepth: -1 })),
    ]
  }
  return {
    ...importer,
    preferredVersions: opts.preferredVersions || importer.manifest && getPreferredVersionsFromPackage(importer.manifest) || {},
    wantedDependencies: wantedDependencies
      .filter(({ alias, updateDepth }) => updateDepth >= 0 || !linkedAliases.has(alias)),
  }
}

const limitLinking = pLimit(16)

function linkBinsOfImporter ({ modulesDir, bin, prefix }: ImporterToLink) {
  const warn = (message: string) => logger.warn({ message, prefix })
  return linkBins(modulesDir, bin, { allowExoticManifests: true, warn })
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

function addDirectDependenciesToLockfile (
  newManifest: ImporterManifest,
  lockfileImporter: LockfileImporter,
  linkedPackages: Array<{alias: string}>,
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
    newLockfileImporter.specifiers[linkedPkg.alias] = getSpecFromPackageManifest(newManifest, linkedPkg.alias)
  })

  const directDependenciesByAlias = directDependencies.reduce((acc, directDependency) => {
    acc[directDependency.alias] = directDependency
    return acc
  }, {})

  const optionalDependencies = R.keys(newManifest.optionalDependencies)
  const dependencies = R.difference(R.keys(newManifest.dependencies), optionalDependencies)
  const devDependencies = R.difference(R.difference(R.keys(newManifest.devDependencies), optionalDependencies), dependencies)
  const allDeps = [
    ...optionalDependencies,
    ...devDependencies,
    ...dependencies,
  ]

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
      newLockfileImporter.specifiers[dep.alias] = getSpecFromPackageManifest(newManifest, dep.alias)
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

  alignDependencyTypes(newManifest, newLockfileImporter)

  return newLockfileImporter
}

function alignDependencyTypes (manifest: ImporterManifest, lockfileImporter: LockfileImporter) {
  const depTypesOfAliases = getAliasToDependencyTypeMap(manifest)

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

function getAliasToDependencyTypeMap (manifest: ImporterManifest) {
  const depTypesOfAliases = {}
  for (const depType of DEPENDENCIES_FIELDS) {
    if (!manifest[depType]) continue
    for (const alias of Object.keys(manifest[depType] || {})) {
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
  return (
    pkgs
    .filter(Boolean) as DependencyManifest[]
  )
    .map(({ name, version }: DependencyManifest) => ({ name, version }))
}
