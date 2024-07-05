import crypto from 'crypto'
import path from 'path'
import { buildModules, type DepsStateCache, linkBinsOfDependencies } from '@pnpm/build-modules'
import { createAllowBuildFunction } from '@pnpm/builder.policy'
import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import {
  LAYOUT_VERSION,
  LOCKFILE_VERSION,
  LOCKFILE_MAJOR_VERSION,
  LOCKFILE_VERSION_V6,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  stageLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import { createBase32HashFromFile } from '@pnpm/crypto.base32-hash'
import { PnpmError } from '@pnpm/error'
import { getContext, type PnpmContext } from '@pnpm/get-context'
import { headlessInstall, type InstallationResultStats } from '@pnpm/headless'
import {
  makeNodeRequireOption,
  runLifecycleHook,
  runLifecycleHooksConcurrently,
  type RunLifecycleHooksConcurrentlyOptions,
} from '@pnpm/lifecycle'
import { linkBins, linkBinsOfPackages } from '@pnpm/link-bins'
import {
  type ProjectSnapshot,
  type Lockfile,
  writeCurrentLockfile,
  writeLockfiles,
  writeWantedLockfile,
  cleanGitBranchLockfiles,
  type PatchFile,
} from '@pnpm/lockfile-file'
import { writePnpFile } from '@pnpm/lockfile-to-pnp'
import { extendProjectsWithTargetDirs, satisfiesPackageManifest } from '@pnpm/lockfile-utils'
import { getPreferredVersionsFromLockfileAndManifests } from '@pnpm/lockfile.preferred-versions'
import { logger, globalInfo, streamParser } from '@pnpm/logger'
import { getAllDependenciesFromManifest, getAllUniqueSpecs } from '@pnpm/manifest-utils'
import { writeModulesManifest } from '@pnpm/modules-yaml'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { removeBin } from '@pnpm/remove-bins'
import {
  getWantedDependencies,
  type DependenciesGraph,
  type DependenciesGraphNode,
  type PinnedVersion,
  resolveDependencies,
  type UpdateMatchingFunction,
  type WantedDependency,
} from '@pnpm/resolve-dependencies'
import {
  type PreferredVersions,
} from '@pnpm/resolver-base'
import {
  type DepPath,
  type DependenciesField,
  type DependencyManifest,
  type PeerDependencyIssues,
  type ProjectId,
  type ProjectManifest,
  type ReadPackageHook,
  type ProjectRootDir,
} from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import isInnerLink from 'is-inner-link'
import isSubdir from 'is-subdir'
import pFilter from 'p-filter'
import pLimit from 'p-limit'
import pMapValues from 'p-map-values'
import mapValues from 'ramda/src/map'
import clone from 'ramda/src/clone'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'
import pipeWith from 'ramda/src/pipeWith'
import props from 'ramda/src/props'
import sortKeys from 'sort-keys'
import { parseWantedDependencies } from '../parseWantedDependencies'
import { removeDeps } from '../uninstall/removeDeps'
import { allProjectsAreUpToDate } from './allProjectsAreUpToDate'
import {
  extendOptions,
  type InstallOptions,
  type ProcessedInstallOptions as StrictInstallOptions,
} from './extendInstallOptions'
import { linkPackages } from './link'
import { reportPeerDependencyIssues } from './reportPeerDependencyIssues'

class LockfileConfigMismatchError extends PnpmError {
  constructor (outdatedLockfileSettingName: string) {
    super('LOCKFILE_CONFIG_MISMATCH',
      `Cannot proceed with the frozen installation. The current "${outdatedLockfileSettingName!}" configuration doesn't match the value found in the lockfile`, {
        hint: 'Update your lockfile using "pnpm install --no-frozen-lockfile"',
      })
  }
}

const BROKEN_LOCKFILE_INTEGRITY_ERRORS = new Set([
  'ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE',
  'ERR_PNPM_TARBALL_INTEGRITY',
])

const DEV_PREINSTALL = 'pnpm:devPreinstall'

interface InstallMutationOptions {
  update?: boolean
  updateMatching?: UpdateMatchingFunction
  updatePackageManifest?: boolean
}

export interface InstallDepsMutation extends InstallMutationOptions {
  mutation: 'install'
  pruneDirectDependencies?: boolean
}

export interface InstallSomeDepsMutation extends InstallMutationOptions {
  allowNew?: boolean
  dependencySelectors: string[]
  mutation: 'installSome'
  peer?: boolean
  pruneDirectDependencies?: boolean
  pinnedVersion?: PinnedVersion
  targetDependenciesField?: DependenciesField
}

export interface UninstallSomeDepsMutation {
  mutation: 'uninstallSome'
  dependencyNames: string[]
  targetDependenciesField?: DependenciesField
}

export interface UnlinkDepsMutation {
  mutation: 'unlink'
}

export interface UnlinkSomeDepsMutation {
  mutation: 'unlinkSome'
  dependencyNames: string[]
}

export type DependenciesMutation = InstallDepsMutation | InstallSomeDepsMutation | UninstallSomeDepsMutation | UnlinkDepsMutation | UnlinkSomeDepsMutation

type Opts = Omit<InstallOptions, 'allProjects'> & {
  preferredVersions?: PreferredVersions
  pruneDirectDependencies?: boolean
} & InstallMutationOptions

export async function install (
  manifest: ProjectManifest,
  opts: Opts
): Promise<ProjectManifest> {
  const rootDir = (opts.dir ?? process.cwd()) as ProjectRootDir
  const { updatedProjects: projects } = await mutateModules(
    [
      {
        mutation: 'install',
        pruneDirectDependencies: opts.pruneDirectDependencies,
        rootDir,
        update: opts.update,
        updateMatching: opts.updateMatching,
        updatePackageManifest: opts.updatePackageManifest,
      },
    ],
    {
      ...opts,
      allProjects: [{
        buildIndex: 0,
        manifest,
        rootDir,
      }],
    }
  )
  return projects[0].manifest
}

interface ProjectToBeInstalled {
  id: string
  buildIndex: number
  manifest: ProjectManifest
  modulesDir: string
  rootDir: ProjectRootDir
}

export type MutatedProject = DependenciesMutation & { rootDir: ProjectRootDir }

export type MutateModulesOptions = InstallOptions & {
  preferredVersions?: PreferredVersions
  hooks?: {
    readPackage?: ReadPackageHook[] | ReadPackageHook
  } | InstallOptions['hooks']
}

export async function mutateModulesInSingleProject (
  project: MutatedProject & {
    binsDir?: string
    manifest: ProjectManifest
    rootDir: ProjectRootDir
    modulesDir?: string
  },
  maybeOpts: Omit<MutateModulesOptions, 'allProjects'> & InstallMutationOptions
): Promise<UpdatedProject> {
  const result = await mutateModules(
    [
      {
        ...project,
        update: maybeOpts.update,
        updateMatching: maybeOpts.updateMatching,
        updatePackageManifest: maybeOpts.updatePackageManifest,
      } as MutatedProject,
    ],
    {
      ...maybeOpts,
      allProjects: [{
        buildIndex: 0,
        ...project,
      }],
    }
  )
  return result.updatedProjects[0]
}

export interface MutateModulesResult {
  updatedProjects: UpdatedProject[]
  stats: InstallationResultStats
}

export async function mutateModules (
  projects: MutatedProject[],
  maybeOpts: MutateModulesOptions
): Promise<MutateModulesResult> {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }

  const opts = extendOptions(maybeOpts)

  if (!opts.include.dependencies && opts.include.optionalDependencies) {
    throw new PnpmError('OPTIONAL_DEPS_REQUIRE_PROD_DEPS', 'Optional dependencies cannot be installed without production dependencies')
  }

  const installsOnly = allMutationsAreInstalls(projects)
  if (!installsOnly) opts.strictPeerDependencies = false
  // @ts-expect-error
  opts['forceNewModules'] = installsOnly
  const rootProjectManifest = opts.allProjects.find(({ rootDir }) => rootDir === opts.lockfileDir)?.manifest ??
    // When running install/update on a subset of projects, the root project might not be included,
    // so reading its manifest explicitly here.
    await safeReadProjectManifestOnly(opts.lockfileDir)

  const ctx = await getContext(opts)

  if (opts.hooks.preResolution) {
    await opts.hooks.preResolution({
      currentLockfile: ctx.currentLockfile,
      wantedLockfile: ctx.wantedLockfile,
      existsCurrentLockfile: ctx.existsCurrentLockfile,
      existsNonEmptyWantedLockfile: ctx.existsNonEmptyWantedLockfile,
      lockfileDir: ctx.lockfileDir,
      storeDir: ctx.storeDir,
      registries: ctx.registries,
    })
  }

  const pruneVirtualStore = ctx.modulesFile?.prunedAt && opts.modulesCacheMaxAge > 0
    ? cacheExpired(ctx.modulesFile.prunedAt, opts.modulesCacheMaxAge)
    : true

  if (!maybeOpts.ignorePackageManifest) {
    for (const { manifest, rootDir } of Object.values(ctx.projects)) {
      if (!manifest) {
        throw new Error(`No package.json found in "${rootDir}"`)
      }
    }
  }

  const result = await _install()

  // @ts-expect-error
  if (global['verifiedFileIntegrity'] > 1000) {
    // @ts-expect-error
    globalInfo(`The integrity of ${global['verifiedFileIntegrity']} files was checked. This might have caused installation to take longer.`)
  }
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  if (opts.mergeGitBranchLockfiles) {
    await cleanGitBranchLockfiles(ctx.lockfileDir)
  }

  return {
    updatedProjects: result.updatedProjects,
    stats: result.stats ?? { added: 0, removed: 0, linkedToRoot: 0 },
  }

  async function _install (): Promise<{ updatedProjects: UpdatedProject[], stats?: InstallationResultStats }> {
    const scriptsOpts: RunLifecycleHooksConcurrentlyOptions = {
      extraBinPaths: opts.extraBinPaths,
      extraNodePaths: ctx.extraNodePaths,
      extraEnv: opts.extraEnv,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      rawConfig: opts.rawConfig,
      resolveSymlinksInInjectedDirs: opts.resolveSymlinksInInjectedDirs,
      scriptsPrependNodePath: opts.scriptsPrependNodePath,
      scriptShell: opts.scriptShell,
      shellEmulator: opts.shellEmulator,
      stdio: opts.ownLifecycleHooksStdio,
      storeController: opts.storeController,
      unsafePerm: opts.unsafePerm || false,
    }

    if (!opts.ignoreScripts && !opts.ignorePackageManifest && rootProjectManifest?.scripts?.[DEV_PREINSTALL]) {
      await runLifecycleHook(
        DEV_PREINSTALL,
        rootProjectManifest,
        {
          ...scriptsOpts,
          depPath: opts.lockfileDir,
          pkgRoot: opts.lockfileDir,
          rootModulesDir: ctx.rootModulesDir,
        }
      )
    }
    const packageExtensionsChecksum = isEmpty(opts.packageExtensions ?? {}) ? undefined : createObjectChecksum(opts.packageExtensions!)
    const pnpmfileChecksum = await opts.hooks.calculatePnpmfileChecksum?.()
    const patchedDependencies = opts.ignorePackageManifest
      ? ctx.wantedLockfile.patchedDependencies
      : (opts.patchedDependencies ? await calcPatchHashes(opts.patchedDependencies, opts.lockfileDir) : {})
    const patchedDependenciesWithResolvedPath = patchedDependencies
      ? mapValues((patchFile) => ({
        hash: patchFile.hash,
        path: path.join(opts.lockfileDir, patchFile.path),
      }), patchedDependencies)
      : undefined
    const frozenLockfile = opts.frozenLockfile ||
      opts.frozenLockfileIfExists && ctx.existsNonEmptyWantedLockfile
    let outdatedLockfileSettings = false
    if (!opts.ignorePackageManifest) {
      const outdatedLockfileSettingName = getOutdatedLockfileSetting(ctx.wantedLockfile, {
        autoInstallPeers: opts.autoInstallPeers,
        excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
        peersSuffixMaxLength: opts.peersSuffixMaxLength,
        overrides: opts.overrides,
        ignoredOptionalDependencies: opts.ignoredOptionalDependencies?.sort(),
        packageExtensionsChecksum,
        patchedDependencies,
        pnpmfileChecksum,
      })
      outdatedLockfileSettings = outdatedLockfileSettingName != null
      if (frozenLockfile && outdatedLockfileSettings) {
        throw new LockfileConfigMismatchError(outdatedLockfileSettingName!)
      }
    }
    const upToDateLockfileMajorVersion = ctx.wantedLockfile.lockfileVersion.toString().startsWith(`${LOCKFILE_MAJOR_VERSION}.`)
    let needsFullResolution = outdatedLockfileSettings ||
      opts.fixLockfile ||
      !upToDateLockfileMajorVersion && ctx.wantedLockfile.lockfileVersion !== LOCKFILE_VERSION_V6 ||
      opts.forceFullResolution
    if (needsFullResolution) {
      ctx.wantedLockfile.settings = {
        autoInstallPeers: opts.autoInstallPeers,
        excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
        peersSuffixMaxLength: opts.peersSuffixMaxLength,
      }
      ctx.wantedLockfile.overrides = opts.overrides
      ctx.wantedLockfile.packageExtensionsChecksum = packageExtensionsChecksum
      ctx.wantedLockfile.ignoredOptionalDependencies = opts.ignoredOptionalDependencies
      ctx.wantedLockfile.pnpmfileChecksum = pnpmfileChecksum
      ctx.wantedLockfile.patchedDependencies = patchedDependencies
    } else if (!frozenLockfile) {
      ctx.wantedLockfile.settings = {
        autoInstallPeers: opts.autoInstallPeers,
        excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
        peersSuffixMaxLength: opts.peersSuffixMaxLength,
      }
    }
    if (
      !ctx.lockfileHadConflicts &&
      !opts.fixLockfile &&
      !opts.dedupe &&
      installsOnly &&
      (
        frozenLockfile ||
        opts.ignorePackageManifest ||
        !needsFullResolution &&
        opts.preferFrozenLockfile &&
        (!opts.pruneLockfileImporters || Object.keys(ctx.wantedLockfile.importers).length === Object.keys(ctx.projects).length) &&
        ctx.existsNonEmptyWantedLockfile &&
        (
          ctx.wantedLockfile.lockfileVersion === LOCKFILE_VERSION ||
          ctx.wantedLockfile.lockfileVersion === LOCKFILE_VERSION_V6 ||
          ctx.wantedLockfile.lockfileVersion === '6.1'
        ) &&
        await allProjectsAreUpToDate(Object.values(ctx.projects), {
          catalogs: opts.catalogs,
          autoInstallPeers: opts.autoInstallPeers,
          excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
          linkWorkspacePackages: opts.linkWorkspacePackagesDepth >= 0,
          wantedLockfile: ctx.wantedLockfile,
          workspacePackages: ctx.workspacePackages,
          lockfileDir: opts.lockfileDir,
        })
      )
    ) {
      if (needsFullResolution) {
        throw new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE',
          'Cannot perform a frozen installation because the version of the lockfile is incompatible with this version of pnpm',
          {
            hint: `Try either:
1. Aligning the version of pnpm that generated the lockfile with the version that installs from it, or
2. Migrating the lockfile so that it is compatible with the newer version of pnpm, or
3. Using "pnpm install --no-frozen-lockfile".
Note that in CI environments, this setting is enabled by default.`,
          }
        )
      }
      if (!opts.ignorePackageManifest) {
        const _satisfiesPackageManifest = satisfiesPackageManifest.bind(null, {
          autoInstallPeers: opts.autoInstallPeers,
          excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
        })
        for (const { id, manifest, rootDir } of Object.values(ctx.projects)) {
          const { satisfies, detailedReason } = _satisfiesPackageManifest(ctx.wantedLockfile.importers[id], manifest)
          if (!satisfies) {
            if (!ctx.existsWantedLockfile) {
              throw new PnpmError('NO_LOCKFILE',
                `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is absent`, {
                  hint: 'Note that in CI environments this setting is true by default. If you still need to run install in such cases, use "pnpm install --no-frozen-lockfile"',
                })
            }

            throw new PnpmError('OUTDATED_LOCKFILE',
              `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with ` +
              path.relative(opts.lockfileDir, path.join(rootDir, 'package.json')), {
                hint: `Note that in CI environments this setting is true by default. If you still need to run install in such cases, use "pnpm install --no-frozen-lockfile"

    Failure reason:
    ${detailedReason ?? ''}`,
              })
          }
        }
      }
      if (opts.lockfileOnly) {
        // The lockfile will only be changed if the workspace will have new projects with no dependencies.
        await writeWantedLockfile(ctx.lockfileDir, ctx.wantedLockfile)
        return {
          updatedProjects: projects.map((mutatedProject) => ctx.projects[mutatedProject.rootDir]),
        }
      }
      if (!ctx.existsNonEmptyWantedLockfile) {
        if (Object.values(ctx.projects).some((project) => pkgHasDependencies(project.manifest))) {
          throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
        }
      } else {
        if (maybeOpts.ignorePackageManifest) {
          logger.info({ message: 'Importing packages to virtual store', prefix: opts.lockfileDir })
        } else {
          logger.info({ message: 'Lockfile is up to date, resolution step is skipped', prefix: opts.lockfileDir })
        }
        try {
          const { stats } = await headlessInstall({
            ...ctx,
            ...opts,
            currentEngine: {
              nodeVersion: opts.nodeVersion,
              pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
            },
            currentHoistedLocations: ctx.modulesFile?.hoistedLocations,
            patchedDependencies: patchedDependenciesWithResolvedPath,
            selectedProjectDirs: projects.map((project) => project.rootDir),
            allProjects: ctx.projects,
            prunedAt: ctx.modulesFile?.prunedAt,
            pruneVirtualStore,
            wantedLockfile: maybeOpts.ignorePackageManifest ? undefined : ctx.wantedLockfile,
            useLockfile: opts.useLockfile && ctx.wantedLockfileIsModified,
          })
          if (
            opts.useLockfile && opts.saveLockfile && opts.mergeGitBranchLockfiles ||
            !upToDateLockfileMajorVersion && !opts.frozenLockfile
          ) {
            await writeLockfiles({
              currentLockfile: ctx.currentLockfile,
              currentLockfileDir: ctx.virtualStoreDir,
              wantedLockfile: ctx.wantedLockfile,
              wantedLockfileDir: ctx.lockfileDir,
              useGitBranchLockfile: opts.useGitBranchLockfile,
              mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
            })
          }
          return {
            updatedProjects: projects.map((mutatedProject) => {
              const project = ctx.projects[mutatedProject.rootDir]
              return {
                ...project,
                manifest: project.originalManifest ?? project.manifest,
              }
            }),
            stats,
          }
        } catch (error: any) { // eslint-disable-line
          if (
            frozenLockfile ||
            (
              error.code !== 'ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY' &&
              !BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code)
            ) ||
            (!ctx.existsNonEmptyWantedLockfile && !ctx.existsCurrentLockfile)
          ) throw error
          if (BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code)) {
            needsFullResolution = true
            // Ideally, we would not update but currently there is no other way to redownload the integrity of the package
            for (const project of projects) {
              (project as InstallMutationOptions).update = true
            }
          }
          // A broken lockfile may be caused by a badly resolved Git conflict
          logger.warn({
            error,
            message: error.message,
            prefix: ctx.lockfileDir,
          })
          logger.error(new PnpmError(error.code, 'The lockfile is broken! Resolution step will be performed to fix it.'))
        }
      }
    }

    const projectsToInstall = [] as ImporterToUpdate[]

    let preferredSpecs: Record<string, string> | null = null

    // TODO: make it concurrent
    /* eslint-disable no-await-in-loop */
    for (const project of projects) {
      const projectOpts = {
        ...project,
        ...ctx.projects[project.rootDir],
      }
      switch (project.mutation) {
      case 'uninstallSome':
        projectsToInstall.push({
          pruneDirectDependencies: false,
          ...projectOpts,
          removePackages: project.dependencyNames,
          updatePackageManifest: true,
          wantedDependencies: [],
        })
        break
      case 'install': {
        await installCase({
          ...projectOpts,
          updatePackageManifest: (projectOpts as InstallDepsMutation).updatePackageManifest ?? (projectOpts as InstallDepsMutation).update,
        })
        break
      }
      case 'installSome': {
        await installSome({
          ...projectOpts,
          updatePackageManifest: (projectOpts as InstallSomeDepsMutation).updatePackageManifest !== false,
        })
        break
      }
      case 'unlink': {
        const packageDirs = await readModulesDir(projectOpts.modulesDir)
        const externalPackages = await pFilter(
          packageDirs!,
          async (packageDir: string) => isExternalLink(ctx.storeDir, projectOpts.modulesDir, packageDir)
        )
        const allDeps = getAllDependenciesFromManifest(projectOpts.manifest)
        const packagesToInstall: string[] = []
        for (const pkgName of externalPackages) {
          await rimraf(path.join(projectOpts.modulesDir, pkgName))
          if (allDeps[pkgName]) {
            packagesToInstall.push(pkgName)
          }
        }
        if (packagesToInstall.length === 0) {
          return {
            updatedProjects: projects.map((mutatedProject) => ctx.projects[mutatedProject.rootDir]),
          }
        }

        // TODO: install only those that were unlinked
        // but don't update their version specs in package.json
        await installCase({ ...projectOpts, mutation: 'install' })
        break
      }
      case 'unlinkSome': {
        if (projectOpts.manifest?.name && opts.globalBin) {
          await removeBin(path.join(opts.globalBin, projectOpts.manifest?.name))
        }
        const packagesToInstall: string[] = []
        const allDeps = getAllDependenciesFromManifest(projectOpts.manifest)
        for (const depName of project.dependencyNames) {
          try {
            if (!await isExternalLink(ctx.storeDir, projectOpts.modulesDir, depName)) {
              logger.warn({
                message: `${depName} is not an external link`,
                prefix: project.rootDir,
              })
              continue
            }
          } catch (err: any) { // eslint-disable-line
            if (err['code'] !== 'ENOENT') throw err
          }
          await rimraf(path.join(projectOpts.modulesDir, depName))
          if (allDeps[depName]) {
            packagesToInstall.push(depName)
          }
        }
        if (packagesToInstall.length === 0) {
          return {
            updatedProjects: projects.map((mutatedProject) => ctx.projects[mutatedProject.rootDir]),
          }
        }

        // TODO: install only those that were unlinked
        // but don't update their version specs in package.json
        await installSome({
          ...projectOpts,
          dependencySelectors: packagesToInstall,
          mutation: 'installSome',
          updatePackageManifest: false,
        })
        break
      }
      }
    }
    /* eslint-enable no-await-in-loop */

    function isWantedDepPrefSame (alias: string, prevPref: string | undefined, nextPref: string): boolean {
      if (prevPref !== nextPref) {
        return false
      }

      // When pnpm catalogs are used, the specifiers can be the same (e.g.
      // "catalog:default"), but the wanted versions for the dependency can be
      // different after resolution if the catalog config was just edited.
      const catalogName = parseCatalogProtocol(prevPref)

      // If there's no catalog name, the catalog protocol was not used and we
      // can assume the pref is the same since prevPref and nextPref match.
      if (catalogName === null) {
        return true
      }

      const prevCatalogEntrySpec = ctx.wantedLockfile.catalogs?.[catalogName]?.[alias]?.specifier
      const nextCatalogEntrySpec = opts.catalogs[catalogName]?.[alias]

      return prevCatalogEntrySpec === nextCatalogEntrySpec
    }

    async function installCase (project: any) { // eslint-disable-line
      const wantedDependencies = getWantedDependencies(project.manifest, {
        autoInstallPeers: opts.autoInstallPeers,
        includeDirect: opts.includeDirect,
        updateWorkspaceDependencies: project.update,
        nodeExecPath: opts.nodeExecPath,
      })
        .map((wantedDependency) => ({ ...wantedDependency, updateSpec: true, preserveNonSemverVersionSpec: true }))

      if (ctx.wantedLockfile?.importers) {
        forgetResolutionsOfPrevWantedDeps(ctx.wantedLockfile.importers[project.id], wantedDependencies, isWantedDepPrefSame)
      }
      if (opts.ignoreScripts && project.manifest?.scripts &&
        (project.manifest.scripts.preinstall ||
          project.manifest.scripts.install ||
          project.manifest.scripts.postinstall ||
          project.manifest.scripts.prepare)
      ) {
        ctx.pendingBuilds.push(project.id)
      }

      projectsToInstall.push({
        pruneDirectDependencies: false,
        ...project,
        wantedDependencies,
      })
    }

    async function installSome (project: any) { // eslint-disable-line
      const currentPrefs = opts.ignoreCurrentPrefs ? {} : getAllDependenciesFromManifest(project.manifest)
      const optionalDependencies = project.targetDependenciesField ? {} : project.manifest.optionalDependencies || {}
      const devDependencies = project.targetDependenciesField ? {} : project.manifest.devDependencies || {}
      if (preferredSpecs == null) {
        const manifests = []
        for (const versions of ctx.workspacePackages.values()) {
          for (const { manifest } of versions.values()) {
            manifests.push(manifest)
          }
        }
        preferredSpecs = getAllUniqueSpecs(manifests)
      }
      const wantedDeps = parseWantedDependencies(project.dependencySelectors, {
        allowNew: project.allowNew !== false,
        currentPrefs,
        defaultTag: opts.tag,
        dev: project.targetDependenciesField === 'devDependencies',
        devDependencies,
        optional: project.targetDependenciesField === 'optionalDependencies',
        optionalDependencies,
        updateWorkspaceDependencies: project.update,
        preferredSpecs,
        overrides: opts.overrides,
      })
      projectsToInstall.push({
        pruneDirectDependencies: false,
        ...project,
        wantedDependencies: wantedDeps.map(wantedDep => ({ ...wantedDep, isNew: !currentPrefs[wantedDep.alias], updateSpec: true, nodeExecPath: opts.nodeExecPath })),
      })
    }

    // Unfortunately, the private lockfile may differ from the public one.
    // A user might run named installations on a project that has a pnpm-lock.yaml file before running a noop install
    const makePartialCurrentLockfile = !installsOnly && (
      ctx.existsNonEmptyWantedLockfile && !ctx.existsCurrentLockfile ||
      !ctx.currentLockfileIsUpToDate
    )
    const result = await installInContext(projectsToInstall, ctx, {
      ...opts,
      currentLockfileIsUpToDate: !ctx.existsNonEmptyWantedLockfile || ctx.currentLockfileIsUpToDate,
      makePartialCurrentLockfile,
      needsFullResolution,
      pruneVirtualStore,
      scriptsOpts,
      updateLockfileMinorVersion: true,
      patchedDependencies: patchedDependenciesWithResolvedPath,
    })

    return {
      updatedProjects: result.projects,
      stats: result.stats,
    }
  }
}

interface PatchHash {
  hash: string
  path: string
}

async function calcPatchHashes (patches: Record<string, string>, lockfileDir: string): Promise<Record<string, PatchHash>> {
  return pMapValues(async (patchFilePath) => {
    return {
      hash: await createBase32HashFromFile(patchFilePath),
      path: path.relative(lockfileDir, patchFilePath).replaceAll('\\', '/'),
    }
  }, patches)
}

type ChangedField =
  | 'patchedDependencies'
  | 'overrides'
  | 'packageExtensionsChecksum'
  | 'ignoredOptionalDependencies'
  | 'settings.autoInstallPeers'
  | 'settings.excludeLinksFromLockfile'
  | 'settings.peersSuffixMaxLength'
  | 'pnpmfileChecksum'

function getOutdatedLockfileSetting (
  lockfile: Lockfile,
  {
    overrides,
    packageExtensionsChecksum,
    ignoredOptionalDependencies,
    patchedDependencies,
    autoInstallPeers,
    excludeLinksFromLockfile,
    peersSuffixMaxLength,
    pnpmfileChecksum,
  }: {
    overrides?: Record<string, string>
    packageExtensionsChecksum?: string
    patchedDependencies?: Record<string, PatchFile>
    ignoredOptionalDependencies?: string[]
    autoInstallPeers?: boolean
    excludeLinksFromLockfile?: boolean
    peersSuffixMaxLength?: number
    pnpmfileChecksum?: string
  }
): ChangedField | null {
  if (!equals(lockfile.overrides ?? {}, overrides ?? {})) {
    return 'overrides'
  }
  if (lockfile.packageExtensionsChecksum !== packageExtensionsChecksum) {
    return 'packageExtensionsChecksum'
  }
  if (!equals(lockfile.ignoredOptionalDependencies?.sort() ?? [], ignoredOptionalDependencies?.sort() ?? [])) {
    return 'ignoredOptionalDependencies'
  }
  if (!equals(lockfile.patchedDependencies ?? {}, patchedDependencies ?? {})) {
    return 'patchedDependencies'
  }
  if ((lockfile.settings?.autoInstallPeers != null && lockfile.settings.autoInstallPeers !== autoInstallPeers)) {
    return 'settings.autoInstallPeers'
  }
  if (lockfile.settings?.excludeLinksFromLockfile != null && lockfile.settings.excludeLinksFromLockfile !== excludeLinksFromLockfile) {
    return 'settings.excludeLinksFromLockfile'
  }
  if (
    lockfile.settings?.peersSuffixMaxLength != null && lockfile.settings.peersSuffixMaxLength !== peersSuffixMaxLength ||
    lockfile.settings?.peersSuffixMaxLength == null && peersSuffixMaxLength !== 1000
  ) {
    return 'settings.peersSuffixMaxLength'
  }
  if (lockfile.pnpmfileChecksum !== pnpmfileChecksum) {
    return 'pnpmfileChecksum'
  }
  return null
}

export function createObjectChecksum (obj: Record<string, unknown>): string {
  const s = JSON.stringify(sortKeys(obj, { deep: true }))
  return crypto.createHash('md5').update(s).digest('hex')
}

function cacheExpired (prunedAt: string, maxAgeInMinutes: number): boolean {
  return ((Date.now() - new Date(prunedAt).valueOf()) / (1000 * 60)) > maxAgeInMinutes
}

async function isExternalLink (storeDir: string, modules: string, pkgName: string): Promise<boolean> {
  const link = await isInnerLink(modules, pkgName)

  return !link.isInner
}

function pkgHasDependencies (manifest: ProjectManifest): boolean {
  return Boolean(
    (Object.keys(manifest.dependencies ?? {}).length > 0) ||
    Object.keys(manifest.devDependencies ?? {}).length ||
    Object.keys(manifest.optionalDependencies ?? {}).length
  )
}

// If the specifier is new, the old resolution probably does not satisfy it anymore.
// By removing these resolutions we ensure that they are resolved again using the new specs.
function forgetResolutionsOfPrevWantedDeps (
  importer: ProjectSnapshot,
  wantedDeps: WantedDependency[],
  isWantedDepPrefSame: (alias: string, prevPref: string | undefined, nextPref: string) => boolean
): void {
  if (!importer.specifiers) return
  importer.dependencies = importer.dependencies ?? {}
  importer.devDependencies = importer.devDependencies ?? {}
  importer.optionalDependencies = importer.optionalDependencies ?? {}
  for (const { alias, pref } of wantedDeps) {
    if (alias && !isWantedDepPrefSame(alias, importer.specifiers[alias], pref)) {
      if (!importer.dependencies[alias]?.startsWith('link:')) {
        delete importer.dependencies[alias]
      }
      delete importer.devDependencies[alias]
      delete importer.optionalDependencies[alias]
    }
  }
}

function forgetResolutionsOfAllPrevWantedDeps (wantedLockfile: Lockfile): void {
  // Similar to the forgetResolutionsOfPrevWantedDeps function above, we can
  // delete existing resolutions in importers to make sure they're resolved
  // again.
  if ((wantedLockfile.importers != null) && !isEmpty(wantedLockfile.importers)) {
    wantedLockfile.importers = mapValues(
      ({ dependencies, devDependencies, optionalDependencies, ...rest }) => rest,
      wantedLockfile.importers)
  }

  // The resolveDependencies function looks at previous PackageSnapshot
  // dependencies/optionalDependencies blocks and merges them with new resolved
  // deps. Clear the previous PackageSnapshot fields so the newly resolved deps
  // are always used.
  if ((wantedLockfile.packages != null) && !isEmpty(wantedLockfile.packages)) {
    wantedLockfile.packages = mapValues(
      ({ dependencies, optionalDependencies, ...rest }) => rest,
      wantedLockfile.packages)
  }
}

export async function addDependenciesToPackage (
  manifest: ProjectManifest,
  dependencySelectors: string[],
  opts: Omit<InstallOptions, 'allProjects'> & {
    bin?: string
    allowNew?: boolean
    peer?: boolean
    pinnedVersion?: 'major' | 'minor' | 'patch'
    targetDependenciesField?: DependenciesField
  } & InstallMutationOptions
): Promise<ProjectManifest> {
  const rootDir = (opts.dir ?? process.cwd()) as ProjectRootDir
  const { updatedProjects: projects } = await mutateModules(
    [
      {
        allowNew: opts.allowNew,
        dependencySelectors,
        mutation: 'installSome',
        peer: opts.peer,
        pinnedVersion: opts.pinnedVersion,
        rootDir,
        targetDependenciesField: opts.targetDependenciesField,
        update: opts.update,
        updateMatching: opts.updateMatching,
        updatePackageManifest: opts.updatePackageManifest,
      },
    ],
    {
      ...opts,
      lockfileDir: opts.lockfileDir ?? opts.dir,
      allProjects: [
        {
          buildIndex: 0,
          binsDir: opts.bin,
          manifest,
          rootDir,
        },
      ],
    })
  return projects[0].manifest
}

export type ImporterToUpdate = {
  buildIndex: number
  binsDir: string
  id: ProjectId
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  modulesDir: string
  rootDir: ProjectRootDir
  pruneDirectDependencies: boolean
  removePackages?: string[]
  updatePackageManifest: boolean
  wantedDependencies: Array<WantedDependency & { isNew?: boolean, updateSpec?: boolean, preserveNonSemverVersionSpec?: boolean }>
} & DependenciesMutation

export interface UpdatedProject {
  originalManifest?: ProjectManifest
  manifest: ProjectManifest
  peerDependencyIssues?: PeerDependencyIssues
  rootDir: ProjectRootDir
}

interface InstallFunctionResult {
  newLockfile: Lockfile
  projects: UpdatedProject[]
  stats?: InstallationResultStats
}

type InstallFunction = (
  projects: ImporterToUpdate[],
  ctx: PnpmContext,
  opts: Omit<StrictInstallOptions, 'patchedDependencies'> & {
    patchedDependencies?: Record<string, PatchFile>
    makePartialCurrentLockfile: boolean
    needsFullResolution: boolean
    neverBuiltDependencies?: string[]
    onlyBuiltDependencies?: string[]
    overrides?: Record<string, string>
    updateLockfileMinorVersion: boolean
    preferredVersions?: PreferredVersions
    pruneVirtualStore: boolean
    scriptsOpts: RunLifecycleHooksConcurrentlyOptions
    currentLockfileIsUpToDate: boolean
    hoistWorkspacePackages?: boolean
  }
) => Promise<InstallFunctionResult>

const _installInContext: InstallFunction = async (projects, ctx, opts) => {
  // The wanted lockfile is mutated during installation. To compare changes, a
  // deep copy before installation is needed. This copy should represent the
  // original wanted lockfile on disk as close as possible.
  //
  // This object can be quite large. Intentionally avoiding an expensive copy
  // if no lockfileCheck option was passed in.
  const originalLockfileForCheck = opts.lockfileCheck != null
    ? clone(ctx.wantedLockfile)
    : null

  // Aliasing for clarity in boolean expressions below.
  const isInstallationOnlyForLockfileCheck = opts.lockfileCheck != null

  ctx.wantedLockfile.importers = ctx.wantedLockfile.importers || {}
  for (const { id } of projects) {
    if (!ctx.wantedLockfile.importers[id]) {
      ctx.wantedLockfile.importers[id] = { specifiers: {} }
    }
  }
  if (opts.pruneLockfileImporters) {
    const projectIds = new Set(projects.map(({ id }) => id))
    for (const wantedImporter of Object.keys(ctx.wantedLockfile.importers) as ProjectId[]) {
      if (!projectIds.has(wantedImporter)) {
        delete ctx.wantedLockfile.importers[wantedImporter]
      }
    }
  }

  await Promise.all(
    projects
      .map(async (project) => {
        if (project.mutation !== 'uninstallSome') return
        const _removeDeps = async (manifest: ProjectManifest) => removeDeps(manifest, project.dependencyNames, { prefix: project.rootDir, saveType: project.targetDependenciesField })
        project.manifest = await _removeDeps(project.manifest)
        if (project.originalManifest != null) {
          project.originalManifest = await _removeDeps(project.originalManifest)
        }
      })
  )

  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'resolution_started',
  })

  const update = projects.some((project) => (project as InstallMutationOptions).update)
  const preferredVersions = opts.preferredVersions ?? (
    !update
      ? getPreferredVersionsFromLockfileAndManifests(ctx.wantedLockfile.packages, Object.values(ctx.projects).map(({ manifest }) => manifest))
      : undefined
  )
  const forceFullResolution = ctx.wantedLockfile.lockfileVersion !== LOCKFILE_VERSION ||
    !opts.currentLockfileIsUpToDate ||
    opts.force ||
    opts.needsFullResolution ||
    ctx.lockfileHadConflicts ||
    opts.dedupePeerDependents

  // Ignore some fields when fixing lockfile, so these fields can be regenerated
  // and make sure it's up to date
  if (
    opts.fixLockfile &&
    (ctx.wantedLockfile.packages != null) &&
    !isEmpty(ctx.wantedLockfile.packages)
  ) {
    ctx.wantedLockfile.packages = mapValues(({ dependencies, optionalDependencies, resolution }) => ({
      // These fields are needed to avoid losing information of the locked dependencies if these fields are not broken
      // If these fields are broken, they will also be regenerated
      dependencies,
      optionalDependencies,
      resolution,
    }), ctx.wantedLockfile.packages)
  }

  if (opts.dedupe) {
    // Deleting recorded version resolutions from importers and packages. These
    // fields will be regenerated using the preferred versions computed above.
    //
    // This is a bit different from a "full resolution", which completely
    // ignores preferred versions from the lockfile.
    forgetResolutionsOfAllPrevWantedDeps(ctx.wantedLockfile)
  }

  let {
    dependenciesGraph,
    dependenciesByProjectId,
    linkedDependenciesByProjectId,
    newLockfile,
    outdatedDependencies,
    peerDependencyIssuesByProjects,
    wantedToBeSkippedPackageIds,
    waitTillAllFetchingsFinish,
  } = await resolveDependencies(
    projects,
    {
      // In the next major allow build should be just () => true here always
      allowBuild: opts.onlyBuiltDependenciesFile ? () => true : createAllowBuildFunction({ onlyBuiltDependencies: opts.onlyBuiltDependencies, neverBuiltDependencies: opts.neverBuiltDependencies }),
      allowedDeprecatedVersions: opts.allowedDeprecatedVersions,
      allowNonAppliedPatches: opts.allowNonAppliedPatches,
      autoInstallPeers: opts.autoInstallPeers,
      autoInstallPeersFromHighestMatch: opts.autoInstallPeersFromHighestMatch,
      catalogs: opts.catalogs,
      currentLockfile: ctx.currentLockfile,
      defaultUpdateDepth: opts.depth,
      dedupeDirectDeps: opts.dedupeDirectDeps,
      dedupeInjectedDeps: opts.dedupeInjectedDeps,
      dedupePeerDependents: opts.dedupePeerDependents,
      dryRun: opts.lockfileOnly,
      engineStrict: opts.engineStrict,
      excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
      force: opts.force,
      forceFullResolution,
      ignoreScripts: opts.ignoreScripts,
      hooks: {
        readPackage: opts.readPackageHook,
      },
      linkWorkspacePackagesDepth: opts.linkWorkspacePackagesDepth ?? (opts.saveWorkspaceProtocol ? 0 : -1),
      lockfileDir: opts.lockfileDir,
      nodeVersion: opts.nodeVersion,
      pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
      preferWorkspacePackages: opts.preferWorkspacePackages,
      preferredVersions,
      preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
      registries: ctx.registries,
      resolutionMode: opts.resolutionMode,
      saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      storeController: opts.storeController,
      tag: opts.tag,
      updateToLatest: opts.updateToLatest,
      virtualStoreDir: ctx.virtualStoreDir,
      virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
      wantedLockfile: ctx.wantedLockfile,
      workspacePackages: ctx.workspacePackages,
      patchedDependencies: opts.patchedDependencies,
      lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
      resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
      supportedArchitectures: opts.supportedArchitectures,
      peersSuffixMaxLength: opts.peersSuffixMaxLength,
    }
  )
  if (!opts.include.optionalDependencies || !opts.include.devDependencies || !opts.include.dependencies) {
    linkedDependenciesByProjectId = mapValues(
      (linkedDeps) => linkedDeps.filter((linkedDep) =>
        !(
          linkedDep.dev && !opts.include.devDependencies ||
          linkedDep.optional && !opts.include.optionalDependencies ||
          !linkedDep.dev && !linkedDep.optional && !opts.include.dependencies
        )),
      linkedDependenciesByProjectId ?? {}
    )
    for (const { id, manifest } of projects) {
      for (const [alias, depPath] of dependenciesByProjectId[id].entries()) {
        let include!: boolean
        const dep = dependenciesGraph[depPath]
        if (!dep) {
          include = false
        } else {
          const isDev = Boolean(manifest.devDependencies?.[dep.name])
          const isOptional = Boolean(manifest.optionalDependencies?.[dep.name])
          include = !(
            isDev && !opts.include.devDependencies ||
            isOptional && !opts.include.optionalDependencies ||
            !isDev && !isOptional && !opts.include.dependencies
          )
        }
        if (!include) {
          dependenciesByProjectId[id].delete(alias)
        }
      }
    }
  }

  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'resolution_done',
  })

  newLockfile = ((opts.hooks?.afterAllResolved) != null)
    ? await pipeWith(async (f, res) => f(await res), opts.hooks.afterAllResolved as any)(newLockfile) as Lockfile // eslint-disable-line
    : newLockfile

  if (opts.updateLockfileMinorVersion) {
    newLockfile.lockfileVersion = LOCKFILE_VERSION
  }

  const depsStateCache: DepsStateCache = {}
  const lockfileOpts = {
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  }
  let stats: InstallationResultStats | undefined
  if (!opts.lockfileOnly && !isInstallationOnlyForLockfileCheck && opts.enableModulesDir) {
    const result = await linkPackages(
      projects,
      dependenciesGraph,
      {
        currentLockfile: ctx.currentLockfile,
        dedupeDirectDeps: opts.dedupeDirectDeps,
        dependenciesByProjectId,
        depsStateCache,
        disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
        extraNodePaths: ctx.extraNodePaths,
        force: opts.force,
        hoistedDependencies: ctx.hoistedDependencies,
        hoistedModulesDir: ctx.hoistedModulesDir,
        hoistPattern: ctx.hoistPattern,
        ignoreScripts: opts.ignoreScripts,
        include: opts.include,
        linkedDependenciesByProjectId,
        lockfileDir: opts.lockfileDir,
        makePartialCurrentLockfile: opts.makePartialCurrentLockfile,
        outdatedDependencies,
        pruneStore: opts.pruneStore,
        pruneVirtualStore: opts.pruneVirtualStore,
        publicHoistPattern: ctx.publicHoistPattern,
        registries: ctx.registries,
        rootModulesDir: ctx.rootModulesDir,
        sideEffectsCacheRead: opts.sideEffectsCacheRead,
        symlink: opts.symlink,
        skipped: ctx.skipped,
        storeController: opts.storeController,
        virtualStoreDir: ctx.virtualStoreDir,
        virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
        wantedLockfile: newLockfile,
        wantedToBeSkippedPackageIds,
        hoistWorkspacePackages: opts.hoistWorkspacePackages,
      }
    )
    stats = result.stats
    if (opts.enablePnp) {
      const importerNames = Object.fromEntries(
        projects.map(({ manifest, id }) => [id, manifest.name ?? id])
      )
      await writePnpFile(result.currentLockfile, {
        importerNames,
        lockfileDir: ctx.lockfileDir,
        virtualStoreDir: ctx.virtualStoreDir,
        virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
        registries: ctx.registries,
      })
    }

    ctx.pendingBuilds = ctx.pendingBuilds
      .filter((relDepPath) => !result.removedDepPaths.has(relDepPath))

    if (result.newDepPaths?.length) {
      if (opts.ignoreScripts) {
        // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
        ctx.pendingBuilds = ctx.pendingBuilds
          .concat(
            result.newDepPaths.filter((depPath) => dependenciesGraph[depPath].requiresBuild)
          )
      }
      if (!opts.ignoreScripts || Object.keys(opts.patchedDependencies ?? {}).length > 0) {
        // postinstall hooks
        const depPaths = Object.keys(dependenciesGraph) as DepPath[]
        const rootNodes = depPaths.filter((depPath) => dependenciesGraph[depPath].depth === 0)

        let extraEnv: Record<string, string> | undefined = opts.scriptsOpts.extraEnv
        if (opts.enablePnp) {
          extraEnv = {
            ...extraEnv,
            ...makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs')),
          }
        }
        await buildModules(dependenciesGraph, rootNodes, {
          allowBuild: createAllowBuildFunction(opts),
          childConcurrency: opts.childConcurrency,
          depsStateCache,
          depsToBuild: new Set(result.newDepPaths),
          extraBinPaths: ctx.extraBinPaths,
          extraNodePaths: ctx.extraNodePaths,
          extraEnv,
          ignoreScripts: opts.ignoreScripts || opts.ignoreDepScripts,
          lockfileDir: ctx.lockfileDir,
          optional: opts.include.optionalDependencies,
          preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          rawConfig: opts.rawConfig,
          rootModulesDir: ctx.virtualStoreDir,
          scriptsPrependNodePath: opts.scriptsPrependNodePath,
          scriptShell: opts.scriptShell,
          shellEmulator: opts.shellEmulator,
          sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
          storeController: opts.storeController,
          unsafePerm: opts.unsafePerm,
          userAgent: opts.userAgent,
        })
      }
    }

    const binWarn = (prefix: string, message: string) => {
      logger.info({ message, prefix })
    }
    if (result.newDepPaths?.length) {
      const newPkgs = props<DepPath, DependenciesGraphNode>(result.newDepPaths, dependenciesGraph)
      await linkAllBins(newPkgs, dependenciesGraph, {
        extraNodePaths: ctx.extraNodePaths,
        optional: opts.include.optionalDependencies,
        warn: binWarn.bind(null, opts.lockfileDir),
      })
    }

    await Promise.all(projects.map(async (project, index) => {
      let linkedPackages!: string[]
      if (ctx.publicHoistPattern?.length && path.relative(project.rootDir, opts.lockfileDir) === '') {
        const nodeExecPathByAlias = Object.entries(project.manifest.dependenciesMeta ?? {})
          .reduce((prev, [alias, { node }]) => {
            if (node) {
              prev[alias] = node
            }
            return prev
          }, {} as Record<string, string>)
        linkedPackages = await linkBins(project.modulesDir, project.binsDir, {
          allowExoticManifests: true,
          preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          projectManifest: project.manifest,
          nodeExecPathByAlias,
          extraNodePaths: ctx.extraNodePaths,
          warn: binWarn.bind(null, project.rootDir),
        })
      } else {
        const directPkgs = [
          ...props<DepPath, DependenciesGraphNode>(
            Array.from(dependenciesByProjectId[project.id].values()).filter((depPath) => !ctx.skipped.has(depPath)),
            dependenciesGraph
          ),
          ...linkedDependenciesByProjectId[project.id].map(({ pkgId }) => ({
            dir: path.join(project.rootDir, pkgId.substring(5)),
            fetching: undefined,
          })),
        ]
        linkedPackages = await linkBinsOfPackages(
          (
            await Promise.all(
              directPkgs.map(async (dep) => {
                const manifest = (await dep.fetching?.())?.bundledManifest ?? await safeReadProjectManifestOnly(dep.dir)
                let nodeExecPath: string | undefined
                if (manifest?.name) {
                  nodeExecPath = project.manifest.dependenciesMeta?.[manifest.name]?.node
                }
                return {
                  location: dep.dir,
                  manifest,
                  nodeExecPath,
                }
              })
            )
          )
            .filter(({ manifest }) => manifest != null) as Array<{ location: string, manifest: DependencyManifest }>,
          project.binsDir,
          {
            extraNodePaths: ctx.extraNodePaths,
            preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          }
        )
      }
      const projectToInstall = projects[index]
      if (opts.global && projectToInstall.mutation.includes('install')) {
        projectToInstall.wantedDependencies.forEach(pkg => {
          if (!linkedPackages?.includes(pkg.alias)) {
            logger.warn({ message: `${pkg.alias ?? pkg.pref} has no binaries`, prefix: opts.lockfileDir })
          }
        })
      }
    }))

    const projectsWithTargetDirs = extendProjectsWithTargetDirs(projects, newLockfile, {
      virtualStoreDir: ctx.virtualStoreDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    })
    await Promise.all([
      opts.useLockfile && opts.saveLockfile
        ? writeLockfiles({
          currentLockfile: result.currentLockfile,
          currentLockfileDir: ctx.virtualStoreDir,
          wantedLockfile: newLockfile,
          wantedLockfileDir: ctx.lockfileDir,
          ...lockfileOpts,
        })
        : writeCurrentLockfile(ctx.virtualStoreDir, result.currentLockfile),
      (async () => {
        if (result.currentLockfile.packages === undefined && result.removedDepPaths.size === 0) {
          return Promise.resolve()
        }
        const injectedDeps: Record<string, string[]> = {}
        for (const project of projectsWithTargetDirs) {
          if (project.targetDirs.length > 0) {
            injectedDeps[project.id] = project.targetDirs.map((targetDir) => path.relative(opts.lockfileDir, targetDir))
          }
        }
        return writeModulesManifest(ctx.rootModulesDir, {
          ...ctx.modulesFile,
          hoistedDependencies: result.newHoistedDependencies,
          hoistPattern: ctx.hoistPattern,
          included: ctx.include,
          injectedDeps,
          layoutVersion: LAYOUT_VERSION,
          nodeLinker: opts.nodeLinker,
          packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
          pendingBuilds: ctx.pendingBuilds,
          publicHoistPattern: ctx.publicHoistPattern,
          prunedAt: opts.pruneVirtualStore || ctx.modulesFile == null
            ? new Date().toUTCString()
            : ctx.modulesFile.prunedAt,
          registries: ctx.registries,
          skipped: Array.from(ctx.skipped),
          storeDir: ctx.storeDir,
          virtualStoreDir: ctx.virtualStoreDir,
          virtualStoreDirMaxLength: ctx.virtualStoreDirMaxLength,
        }, {
          makeModulesDir: Object.keys(result.currentLockfile.packages ?? {}).length > 0,
        })
      })(),
    ])
    if (!opts.ignoreScripts) {
      if (opts.enablePnp) {
        opts.scriptsOpts.extraEnv = {
          ...opts.scriptsOpts.extraEnv,
          ...makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs')),
        }
      }
      const projectsToBeBuilt = projectsWithTargetDirs.filter(({ mutation }) => mutation === 'install') as ProjectToBeInstalled[]
      await runLifecycleHooksConcurrently(['preinstall', 'install', 'postinstall', 'prepare'],
        projectsToBeBuilt,
        opts.childConcurrency,
        opts.scriptsOpts
      )
    }
  } else {
    if (opts.useLockfile && !isInstallationOnlyForLockfileCheck) {
      await writeWantedLockfile(ctx.lockfileDir, newLockfile, lockfileOpts)
    }

    if (opts.nodeLinker !== 'hoisted') {
      // This is only needed because otherwise the reporter will hang
      stageLogger.debug({
        prefix: opts.lockfileDir,
        stage: 'importing_done',
      })
    }
  }

  await waitTillAllFetchingsFinish()

  summaryLogger.debug({ prefix: opts.lockfileDir })

  reportPeerDependencyIssues(peerDependencyIssuesByProjects, {
    lockfileDir: opts.lockfileDir,
    strictPeerDependencies: opts.strictPeerDependencies,
  })

  // Similar to the sequencing for when the original wanted lockfile is
  // copied, the new lockfile passed here should be as close as possible to
  // what will eventually be written to disk. Ex: peers should be resolved,
  // the afterAllResolved hook has been applied, etc.
  if (originalLockfileForCheck != null) {
    opts.lockfileCheck?.(originalLockfileForCheck, newLockfile)
  }

  return {
    newLockfile,
    projects: projects.map(({ id, manifest, rootDir }) => ({
      manifest,
      peerDependencyIssues: peerDependencyIssuesByProjects[id],
      rootDir,
    })),
    stats,
  }
}

function allMutationsAreInstalls (projects: MutatedProject[]): boolean {
  return projects.every((project) => project.mutation === 'install' && !project.update && !project.updateMatching)
}

const installInContext: InstallFunction = async (projects, ctx, opts) => {
  try {
    const isPathInsideWorkspace = isSubdir.bind(null, opts.lockfileDir)
    if (!opts.frozenLockfile && opts.useLockfile) {
      const allProjectsLocatedInsideWorkspace = Object.values(ctx.projects)
        .filter((project) => isPathInsideWorkspace(project.rootDirRealPath ?? project.rootDir))
      if (allProjectsLocatedInsideWorkspace.length > projects.length) {
        if (
          allMutationsAreInstalls(projects) &&
          await allProjectsAreUpToDate(allProjectsLocatedInsideWorkspace, {
            catalogs: opts.catalogs,
            autoInstallPeers: opts.autoInstallPeers,
            excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
            linkWorkspacePackages: opts.linkWorkspacePackagesDepth >= 0,
            wantedLockfile: ctx.wantedLockfile,
            workspacePackages: ctx.workspacePackages,
            lockfileDir: opts.lockfileDir,
          })
        ) {
          return installInContext(projects, ctx, {
            ...opts,
            frozenLockfile: true,
          })
        } else {
          const newProjects = [...projects]
          const getWantedDepsOpts = {
            autoInstallPeers: opts.autoInstallPeers,
            includeDirect: opts.includeDirect,
            updateWorkspaceDependencies: false,
            nodeExecPath: opts.nodeExecPath,
          }
          for (const project of allProjectsLocatedInsideWorkspace) {
            if (!newProjects.some(({ rootDir }) => rootDir === project.rootDir)) {
              const wantedDependencies = getWantedDependencies(project.manifest, getWantedDepsOpts)
                .map((wantedDependency) => ({ ...wantedDependency, updateSpec: true, preserveNonSemverVersionSpec: true }))
              newProjects.push({
                mutation: 'install',
                ...project,
                wantedDependencies,
                pruneDirectDependencies: false,
                updatePackageManifest: false,
              })
            }
          }
          const result = await installInContext(newProjects, ctx, {
            ...opts,
            lockfileOnly: true,
          })
          const { stats } = await headlessInstall({
            ...ctx,
            ...opts,
            currentEngine: {
              nodeVersion: opts.nodeVersion,
              pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
            },
            currentHoistedLocations: ctx.modulesFile?.hoistedLocations,
            selectedProjectDirs: projects.map((project) => project.rootDir),
            allProjects: ctx.projects,
            prunedAt: ctx.modulesFile?.prunedAt,
            wantedLockfile: result.newLockfile,
            useLockfile: opts.useLockfile && ctx.wantedLockfileIsModified,
            hoistWorkspacePackages: opts.hoistWorkspacePackages,
          })
          return {
            ...result,
            stats,
          }
        }
      }
    }
    if (opts.nodeLinker === 'hoisted' && !opts.lockfileOnly) {
      const result = await _installInContext(projects, ctx, {
        ...opts,
        lockfileOnly: true,
      })
      const { stats } = await headlessInstall({
        ...ctx,
        ...opts,
        currentEngine: {
          nodeVersion: opts.nodeVersion,
          pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
        },
        currentHoistedLocations: ctx.modulesFile?.hoistedLocations,
        selectedProjectDirs: projects.map((project) => project.rootDir),
        allProjects: ctx.projects,
        prunedAt: ctx.modulesFile?.prunedAt,
        wantedLockfile: result.newLockfile,
        useLockfile: opts.useLockfile && ctx.wantedLockfileIsModified,
        hoistWorkspacePackages: opts.hoistWorkspacePackages,
      })
      return {
        ...result,
        stats,
      }
    }
    if (opts.lockfileOnly && ctx.existsCurrentLockfile) {
      logger.warn({
        message: '`node_modules` is present. Lockfile only installation will make it out-of-date',
        prefix: ctx.lockfileDir,
      })
    }
    return await _installInContext(projects, ctx, opts)
  } catch (error: any) { // eslint-disable-line
    if (
      !BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code) ||
      (!ctx.existsNonEmptyWantedLockfile && !ctx.existsCurrentLockfile)
    ) throw error
    opts.needsFullResolution = true
    // Ideally, we would not update but currently there is no other way to redownload the integrity of the package
    for (const project of projects) {
      (project as InstallMutationOptions).update = true
    }
    logger.warn({
      error,
      message: error.message,
      prefix: ctx.lockfileDir,
    })
    logger.error(new PnpmError(error.code, 'The lockfile is broken! A full installation will be performed in an attempt to fix it.'))
    return _installInContext(projects, ctx, opts)
  } finally {
    await opts.storeController.close()
  }
}

const limitLinking = pLimit(16)

async function linkAllBins (
  depNodes: DependenciesGraphNode[],
  depGraph: DependenciesGraph,
  opts: {
    extraNodePaths?: string[]
    preferSymlinkedExecutables?: boolean
    optional: boolean
    warn: (message: string) => void
  }
): Promise<void> {
  await Promise.all(
    depNodes.map(async depNode => limitLinking(async () => linkBinsOfDependencies(depNode, depGraph, opts)))
  )
}
