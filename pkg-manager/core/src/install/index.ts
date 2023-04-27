import crypto from 'crypto'
import path from 'path'
import { buildModules, type DepsStateCache, linkBinsOfDependencies } from '@pnpm/build-modules'
import {
  LAYOUT_VERSION,
  LOCKFILE_VERSION,
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
import { headlessInstall } from '@pnpm/headless'
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
import { extendProjectsWithTargetDirs } from '@pnpm/lockfile-utils'
import { logger, globalInfo, streamParser } from '@pnpm/logger'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
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
  type DependenciesField,
  type DependencyManifest,
  type PeerDependencyIssues,
  type ProjectManifest,
  type ReadPackageHook,
} from '@pnpm/types'
import rimraf from '@zkochan/rimraf'
import isInnerLink from 'is-inner-link'
import pFilter from 'p-filter'
import pLimit from 'p-limit'
import pMapValues from 'p-map-values'
import flatten from 'ramda/src/flatten'
import mapValues from 'ramda/src/map'
import clone from 'ramda/src/clone'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'
import pickBy from 'ramda/src/pickBy'
import pipeWith from 'ramda/src/pipeWith'
import props from 'ramda/src/props'
import unnest from 'ramda/src/unnest'
import { parseWantedDependencies } from '../parseWantedDependencies'
import { removeDeps } from '../uninstall/removeDeps'
import { allProjectsAreUpToDate } from './allProjectsAreUpToDate'
import {
  extendOptions,
  type InstallOptions,
  type ProcessedInstallOptions as StrictInstallOptions,
} from './extendInstallOptions'
import { getAllUniqueSpecs, getPreferredVersionsFromLockfileAndManifests } from './getPreferredVersions'
import { linkPackages } from './link'
import { reportPeerDependencyIssues } from './reportPeerDependencyIssues'

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

export async function install (
  manifest: ProjectManifest,
  opts: Omit<InstallOptions, 'allProjects'> & {
    preferredVersions?: PreferredVersions
    pruneDirectDependencies?: boolean
  } & InstallMutationOptions
) {
  const rootDir = opts.dir ?? process.cwd()
  const projects = await mutateModules(
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
  rootDir: string
}

export type MutatedProject = DependenciesMutation & { rootDir: string }

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
    rootDir: string
    modulesDir?: string
  },
  maybeOpts: Omit<MutateModulesOptions, 'allProjects'> & InstallMutationOptions
): Promise<UpdatedProject> {
  const [updatedProject] = await mutateModules(
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
  return updatedProject
}

export async function mutateModules (
  projects: MutatedProject[],
  maybeOpts: MutateModulesOptions
): Promise<UpdatedProject[]> {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }

  const opts = await extendOptions(maybeOpts)

  if (!opts.include.dependencies && opts.include.optionalDependencies) {
    throw new PnpmError('OPTIONAL_DEPS_REQUIRE_PROD_DEPS', 'Optional dependencies cannot be installed without production dependencies')
  }

  const installsOnly = projects.every((project) => project.mutation === 'install' && !project.update && !project.updateMatching)
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
      existsWantedLockfile: ctx.existsWantedLockfile,
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
    globalInfo(`The integrity of ${global['verifiedFileIntegrity']} files was checked. This might have caused installation to take longer.`) // eslint-disable-line
  }
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  if (opts.mergeGitBranchLockfiles) {
    await cleanGitBranchLockfiles(ctx.lockfileDir)
  }

  return result

  async function _install (): Promise<UpdatedProject[]> {
    const scriptsOpts: RunLifecycleHooksConcurrentlyOptions = {
      extraBinPaths: opts.extraBinPaths,
      extraEnv: opts.extraEnv,
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
    const patchedDependencies = opts.ignorePackageManifest
      ? ctx.wantedLockfile.patchedDependencies
      : (opts.patchedDependencies ? await calcPatchHashes(opts.patchedDependencies, opts.lockfileDir) : {})
    const patchedDependenciesWithResolvedPath = patchedDependencies
      ? mapValues((patchFile) => ({
        hash: patchFile.hash,
        path: path.join(opts.lockfileDir, patchFile.path),
      }), patchedDependencies)
      : undefined
    let needsFullResolution = !maybeOpts.ignorePackageManifest &&
      lockfileIsNotUpToDate(ctx.wantedLockfile, {
        overrides: opts.overrides,
        neverBuiltDependencies: opts.neverBuiltDependencies,
        onlyBuiltDependencies: opts.onlyBuiltDependencies,
        packageExtensionsChecksum,
        patchedDependencies,
      }) ||
      opts.fixLockfile ||
      !ctx.wantedLockfile.lockfileVersion.toString().startsWith('6.') ||
      opts.forceFullResolution
    if (needsFullResolution) {
      ctx.wantedLockfile.overrides = opts.overrides
      ctx.wantedLockfile.neverBuiltDependencies = opts.neverBuiltDependencies
      ctx.wantedLockfile.onlyBuiltDependencies = opts.onlyBuiltDependencies
      ctx.wantedLockfile.packageExtensionsChecksum = packageExtensionsChecksum
      ctx.wantedLockfile.patchedDependencies = patchedDependencies
    }
    const frozenLockfile = opts.frozenLockfile ||
      opts.frozenLockfileIfExists && ctx.existsWantedLockfile
    if (
      !ctx.lockfileHadConflicts &&
      !opts.fixLockfile &&
      !opts.dedupe &&
      installsOnly &&
      (
        frozenLockfile && !opts.lockfileOnly ||
        opts.ignorePackageManifest ||
        !needsFullResolution &&
        opts.preferFrozenLockfile &&
        (!opts.pruneLockfileImporters || Object.keys(ctx.wantedLockfile.importers).length === Object.keys(ctx.projects).length) &&
        ctx.existsWantedLockfile &&
        (
          ctx.wantedLockfile.lockfileVersion === LOCKFILE_VERSION ||
          ctx.wantedLockfile.lockfileVersion === LOCKFILE_VERSION_V6
        ) &&
        await allProjectsAreUpToDate(Object.values(ctx.projects), {
          autoInstallPeers: opts.autoInstallPeers,
          excludeLinksFromLockfile: opts.excludeLinksFromLockfile,
          linkWorkspacePackages: opts.linkWorkspacePackagesDepth >= 0,
          wantedLockfile: ctx.wantedLockfile,
          workspacePackages: opts.workspacePackages,
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
      if (opts.lockfileOnly) {
        // The lockfile will only be changed if the workspace will have new projects with no dependencies.
        await writeWantedLockfile(ctx.lockfileDir, ctx.wantedLockfile)
        return projects.map((mutatedProject) => ctx.projects[mutatedProject.rootDir])
      }
      if (!ctx.existsWantedLockfile) {
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
          await headlessInstall({
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
          if (opts.useLockfile && opts.saveLockfile && opts.mergeGitBranchLockfiles) {
            await writeLockfiles({
              currentLockfile: ctx.currentLockfile,
              currentLockfileDir: ctx.virtualStoreDir,
              wantedLockfile: ctx.wantedLockfile,
              wantedLockfileDir: ctx.lockfileDir,
              forceSharedFormat: opts.forceSharedLockfile,
              useGitBranchLockfile: opts.useGitBranchLockfile,
              mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
            })
          }
          return projects.map((mutatedProject) => {
            const project = ctx.projects[mutatedProject.rootDir]
            return {
              ...project,
              manifest: project.originalManifest ?? project.manifest,
            }
          })
        } catch (error: any) { // eslint-disable-line
          if (
            frozenLockfile ||
            (
              error.code !== 'ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY' &&
              !BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code)
            ) ||
            (!ctx.existsWantedLockfile && !ctx.existsCurrentLockfile)
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
          return projects.map((mutatedProject) => ctx.projects[mutatedProject.rootDir])
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
          return projects.map((mutatedProject) => ctx.projects[mutatedProject.rootDir])
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

    async function installCase (project: any) { // eslint-disable-line
      const wantedDependencies = getWantedDependencies(project.manifest, {
        autoInstallPeers: opts.autoInstallPeers,
        includeDirect: opts.includeDirect,
        updateWorkspaceDependencies: project.update,
        nodeExecPath: opts.nodeExecPath,
      })
        .map((wantedDependency) => ({ ...wantedDependency, updateSpec: true, preserveNonSemverVersionSpec: true }))

      if (ctx.wantedLockfile?.importers) {
        forgetResolutionsOfPrevWantedDeps(ctx.wantedLockfile.importers[project.id], wantedDependencies)
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
        preferredSpecs = getAllUniqueSpecs(flatten(Object.values(opts.workspacePackages).map(obj => Object.values(obj))).map(({ manifest }) => manifest))
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
      ctx.existsWantedLockfile && !ctx.existsCurrentLockfile ||
      !ctx.currentLockfileIsUpToDate
    )
    const result = await installInContext(projectsToInstall, ctx, {
      ...opts,
      currentLockfileIsUpToDate: !ctx.existsWantedLockfile || ctx.currentLockfileIsUpToDate,
      makePartialCurrentLockfile,
      needsFullResolution,
      pruneVirtualStore,
      scriptsOpts,
      updateLockfileMinorVersion: true,
      patchedDependencies: patchedDependenciesWithResolvedPath,
    })

    return result.projects
  }
}

async function calcPatchHashes (patches: Record<string, string>, lockfileDir: string) {
  return pMapValues(async (patchFileRelativePath) => {
    const patchFilePath = path.join(lockfileDir, patchFileRelativePath)
    return {
      hash: await createBase32HashFromFile(patchFilePath),
      path: patchFileRelativePath,
    }
  }, patches)
}

function lockfileIsNotUpToDate (
  lockfile: Lockfile,
  {
    neverBuiltDependencies,
    onlyBuiltDependencies,
    overrides,
    packageExtensionsChecksum,
    patchedDependencies,
  }: {
    neverBuiltDependencies?: string[]
    onlyBuiltDependencies?: string[]
    overrides?: Record<string, string>
    packageExtensionsChecksum?: string
    patchedDependencies?: Record<string, PatchFile>
  }) {
  return !equals(lockfile.overrides ?? {}, overrides ?? {}) ||
    !equals((lockfile.neverBuiltDependencies ?? []).sort(), (neverBuiltDependencies ?? []).sort()) ||
    !equals(onlyBuiltDependencies?.sort(), lockfile.onlyBuiltDependencies) ||
    lockfile.packageExtensionsChecksum !== packageExtensionsChecksum ||
    !equals(lockfile.patchedDependencies ?? {}, patchedDependencies ?? {})
}

export function createObjectChecksum (obj: unknown) {
  const s = JSON.stringify(obj)
  return crypto.createHash('md5').update(s).digest('hex')
}

function cacheExpired (prunedAt: string, maxAgeInMinutes: number) {
  return ((Date.now() - new Date(prunedAt).valueOf()) / (1000 * 60)) > maxAgeInMinutes
}

async function isExternalLink (storeDir: string, modules: string, pkgName: string) {
  const link = await isInnerLink(modules, pkgName)

  return !link.isInner
}

function pkgHasDependencies (manifest: ProjectManifest) {
  return Boolean(
    (Object.keys(manifest.dependencies ?? {}).length > 0) ||
    Object.keys(manifest.devDependencies ?? {}).length ||
    Object.keys(manifest.optionalDependencies ?? {}).length
  )
}

// If the specifier is new, the old resolution probably does not satisfy it anymore.
// By removing these resolutions we ensure that they are resolved again using the new specs.
function forgetResolutionsOfPrevWantedDeps (importer: ProjectSnapshot, wantedDeps: WantedDependency[]) {
  if (!importer.specifiers) return
  importer.dependencies = importer.dependencies ?? {}
  importer.devDependencies = importer.devDependencies ?? {}
  importer.optionalDependencies = importer.optionalDependencies ?? {}
  for (const { alias, pref } of wantedDeps) {
    if (alias && importer.specifiers[alias] !== pref) {
      if (!importer.dependencies[alias]?.startsWith('link:')) {
        delete importer.dependencies[alias]
      }
      delete importer.devDependencies[alias]
      delete importer.optionalDependencies[alias]
    }
  }
}

function forgetResolutionsOfAllPrevWantedDeps (wantedLockfile: Lockfile) {
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
) {
  const rootDir = opts.dir ?? process.cwd()
  const projects = await mutateModules(
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
  id: string
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  modulesDir: string
  rootDir: string
  pruneDirectDependencies: boolean
  removePackages?: string[]
  updatePackageManifest: boolean
  wantedDependencies: Array<WantedDependency & { isNew?: boolean, updateSpec?: boolean, preserveNonSemverVersionSpec?: boolean }>
} & DependenciesMutation

export interface UpdatedProject {
  originalManifest?: ProjectManifest
  manifest: ProjectManifest
  peerDependencyIssues?: PeerDependencyIssues
  rootDir: string
}

interface InstallFunctionResult {
  newLockfile: Lockfile
  projects: UpdatedProject[]
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
  }
) => Promise<InstallFunctionResult>

const _installInContext: InstallFunction = async (projects, ctx, opts) => {
  if (opts.lockfileOnly && ctx.existsCurrentLockfile) {
    logger.warn({
      message: '`node_modules` is present. Lockfile only installation will make it out-of-date',
      prefix: ctx.lockfileDir,
    })
  }

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
    for (const wantedImporter of Object.keys(ctx.wantedLockfile.importers)) {
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
    finishLockfileUpdates,
    linkedDependenciesByProjectId,
    newLockfile,
    outdatedDependencies,
    peerDependencyIssuesByProjects,
    wantedToBeSkippedPackageIds,
    waitTillAllFetchingsFinish,
  } = await resolveDependencies(
    projects,
    {
      allowBuild: createAllowBuildFunction(opts),
      allowedDeprecatedVersions: opts.allowedDeprecatedVersions,
      allowNonAppliedPatches: opts.allowNonAppliedPatches,
      autoInstallPeers: opts.autoInstallPeers,
      currentLockfile: ctx.currentLockfile,
      defaultUpdateDepth: opts.depth,
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
      virtualStoreDir: ctx.virtualStoreDir,
      wantedLockfile: ctx.wantedLockfile,
      workspacePackages: opts.workspacePackages,
      patchedDependencies: opts.patchedDependencies,
      lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
      resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
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
      dependenciesByProjectId[id] = pickBy((depPath) => {
        const dep = dependenciesGraph[depPath]
        if (!dep) return false
        const isDev = Boolean(manifest.devDependencies?.[dep.name])
        const isOptional = Boolean(manifest.optionalDependencies?.[dep.name])
        return !(
          isDev && !opts.include.devDependencies ||
          isOptional && !opts.include.optionalDependencies ||
          !isDev && !isOptional && !opts.include.dependencies
        )
      }, dependenciesByProjectId[id])
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
    newLockfile.lockfileVersion = LOCKFILE_VERSION_V6
  }

  const depsStateCache: DepsStateCache = {}
  const lockfileOpts = {
    forceSharedFormat: opts.forceSharedLockfile,
    useInlineSpecifiersFormat: true,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    mergeGitBranchLockfiles: opts.mergeGitBranchLockfiles,
  }
  if (!opts.lockfileOnly && !isInstallationOnlyForLockfileCheck && opts.enableModulesDir) {
    const result = await linkPackages(
      projects,
      dependenciesGraph,
      {
        currentLockfile: ctx.currentLockfile,
        dedupeDirectDeps: opts.dedupeDirectDeps,
        dependenciesByProjectId,
        depsStateCache,
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
        wantedLockfile: newLockfile,
        wantedToBeSkippedPackageIds,
      }
    )
    await finishLockfileUpdates()
    if (opts.enablePnp) {
      const importerNames = Object.fromEntries(
        projects.map(({ manifest, id }) => [id, manifest.name ?? id])
      )
      await writePnpFile(result.currentLockfile, {
        importerNames,
        lockfileDir: ctx.lockfileDir,
        virtualStoreDir: ctx.virtualStoreDir,
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
            await pFilter(result.newDepPaths,
              (depPath) => {
                const requiresBuild = dependenciesGraph[depPath].requiresBuild
                if (typeof requiresBuild === 'function') return requiresBuild()
                return requiresBuild
              }
            )
          )
      }
      if (!opts.ignoreScripts || Object.keys(opts.patchedDependencies ?? {}).length > 0) {
        // postinstall hooks
        const depPaths = Object.keys(dependenciesGraph)
        const rootNodes = depPaths.filter((depPath) => dependenciesGraph[depPath].depth === 0)

        let extraEnv: Record<string, string> | undefined = opts.scriptsOpts.extraEnv
        if (opts.enablePnp) {
          extraEnv = {
            ...extraEnv,
            ...makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs')),
          }
        }
        await buildModules(dependenciesGraph, rootNodes, {
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
      const newPkgs = props<string, DependenciesGraphNode>(result.newDepPaths, dependenciesGraph)
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
          ...props<string, DependenciesGraphNode>(
            Object.values(dependenciesByProjectId[project.id]).filter((depPath) => !ctx.skipped.has(depPath)),
            dependenciesGraph
          ),
          ...linkedDependenciesByProjectId[project.id].map(({ pkgId }) => ({
            dir: path.join(project.rootDir, pkgId.substring(5)),
            fetchingBundledManifest: undefined,
          })),
        ]
        linkedPackages = await linkBinsOfPackages(
          (
            await Promise.all(
              directPkgs.map(async (dep) => {
                const manifest = await dep.fetchingBundledManifest?.() ?? await safeReadProjectManifestOnly(dep.dir)
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

    const projectsWithTargetDirs = extendProjectsWithTargetDirs(projects, newLockfile, ctx)
    await Promise.all([
      opts.useLockfile && opts.saveLockfile
        ? writeLockfiles({
          currentLockfile: result.currentLockfile,
          currentLockfileDir: ctx.virtualStoreDir,
          wantedLockfile: newLockfile,
          wantedLockfileDir: ctx.lockfileDir,
          ...lockfileOpts,
        })
        : writeCurrentLockfile(ctx.virtualStoreDir, result.currentLockfile, lockfileOpts),
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
    await finishLockfileUpdates()
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

  await opts.storeController.close()

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
  }
}

function createAllowBuildFunction (
  opts: {
    neverBuiltDependencies?: string[]
    onlyBuiltDependencies?: string[]
  }
): undefined | ((pkgName: string) => boolean) {
  if (opts.neverBuiltDependencies != null && opts.neverBuiltDependencies.length > 0) {
    const neverBuiltDependencies = new Set(opts.neverBuiltDependencies)
    return (pkgName) => !neverBuiltDependencies.has(pkgName)
  } else if (opts.onlyBuiltDependencies != null) {
    const onlyBuiltDependencies = new Set(opts.onlyBuiltDependencies)
    return (pkgName) => onlyBuiltDependencies.has(pkgName)
  }
  return undefined
}

const installInContext: InstallFunction = async (projects, ctx, opts) => {
  try {
    if (opts.nodeLinker === 'hoisted' && !opts.lockfileOnly) {
      const result = await _installInContext(projects, ctx, {
        ...opts,
        lockfileOnly: true,
      })
      await headlessInstall({
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
      })
      return result
    }
    return await _installInContext(projects, ctx, opts)
  } catch (error: any) { // eslint-disable-line
    if (
      !BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code) ||
      (!ctx.existsWantedLockfile && !ctx.existsCurrentLockfile)
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
) {
  return unnest(await Promise.all(
    depNodes.map(async depNode => limitLinking(async () => linkBinsOfDependencies(depNode, depGraph, opts)))
  ))
}
