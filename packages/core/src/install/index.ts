import crypto from 'crypto'
import path from 'path'
import buildModules, { DepsStateCache, linkBinsOfDependencies } from '@pnpm/build-modules'
import {
  LAYOUT_VERSION,
  LOCKFILE_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  stageLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import PnpmError from '@pnpm/error'
import getContext, { PnpmContext, ProjectOptions } from '@pnpm/get-context'
import headless, { Project } from '@pnpm/headless'
import runLifecycleHook, {
  makeNodeRequireOption,
  runLifecycleHooksConcurrently,
  RunLifecycleHooksConcurrentlyOptions,
} from '@pnpm/lifecycle'
import linkBins, { linkBinsOfPackages } from '@pnpm/link-bins'
import {
  ProjectSnapshot,
  Lockfile,
  writeCurrentLockfile,
  writeLockfiles,
  writeWantedLockfile,
} from '@pnpm/lockfile-file'
import { writePnpFile } from '@pnpm/lockfile-to-pnp'
import { extendProjectsWithTargetDirs } from '@pnpm/lockfile-utils'
import logger, { globalWarn, streamParser } from '@pnpm/logger'
import { getAllDependenciesFromManifest } from '@pnpm/manifest-utils'
import { write as writeModulesYaml } from '@pnpm/modules-yaml'
import readModulesDirs from '@pnpm/read-modules-dir'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { removeBin } from '@pnpm/remove-bins'
import resolveDependencies, {
  getWantedDependencies,
  DependenciesGraph,
  DependenciesGraphNode,
  PinnedVersion,
  WantedDependency,
} from '@pnpm/resolve-dependencies'
import {
  PreferredVersions,
} from '@pnpm/resolver-base'
import {
  DependenciesField,
  DependencyManifest,
  PackageExtension,
  PeerDependencyIssues,
  PeerDependencyRules,
  ProjectManifest,
  ReadPackageHook,
} from '@pnpm/types'
import { packageExtensions as compatPackageExtensions } from '@yarnpkg/extensions'
import rimraf from '@zkochan/rimraf'
import isInnerLink from 'is-inner-link'
import pFilter from 'p-filter'
import pLimit from 'p-limit'
import flatten from 'ramda/src/flatten'
import fromPairs from 'ramda/src/fromPairs'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'
import pipeWith from 'ramda/src/pipeWith'
import props from 'ramda/src/props'
import unnest from 'ramda/src/unnest'
import parseWantedDependencies from '../parseWantedDependencies'
import removeDeps from '../uninstall/removeDeps'
import allProjectsAreUpToDate from './allProjectsAreUpToDate'
import createPackageExtender from './createPackageExtender'
import createVersionsOverrider from './createVersionsOverrider'
import createPeerDependencyPatcher from './createPeerDependencyPatcher'
import extendOptions, {
  InstallOptions,
  StrictInstallOptions,
} from './extendInstallOptions'
import { getPreferredVersionsFromLockfile, getAllUniqueSpecs } from './getPreferredVersions'
import linkPackages from './link'
import reportPeerDependencyIssues from './reportPeerDependencyIssues'

const BROKEN_LOCKFILE_INTEGRITY_ERRORS = new Set([
  'ERR_PNPM_UNEXPECTED_PKG_CONTENT_IN_STORE',
  'ERR_PNPM_TARBALL_INTEGRITY',
])

const DEV_PREINSTALL = 'pnpm:devPreinstall'

export type DependenciesMutation = (
  {
    buildIndex: number
    mutation: 'install'
    pruneDirectDependencies?: boolean
  } | {
    allowNew?: boolean
    dependencySelectors: string[]
    mutation: 'installSome'
    peer?: boolean
    pruneDirectDependencies?: boolean
    pinnedVersion?: PinnedVersion
    targetDependenciesField?: DependenciesField
  } | {
    mutation: 'uninstallSome'
    dependencyNames: string[]
    targetDependenciesField?: DependenciesField
  } | {
    mutation: 'unlink'
  } | {
    mutation: 'unlinkSome'
    dependencyNames: string[]
  }
) & (
  {
    manifest: ProjectManifest
  }
)

export async function install (
  manifest: ProjectManifest,
  opts: InstallOptions & {
    preferredVersions?: PreferredVersions
    pruneDirectDependencies?: boolean
  }
) {
  const projects = await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        pruneDirectDependencies: opts.pruneDirectDependencies,
        rootDir: opts.dir ?? process.cwd(),
      },
    ],
    opts
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

export type MutatedProject = ProjectOptions & DependenciesMutation

export type MutateModulesOptions = InstallOptions & {
  preferredVersions?: PreferredVersions
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

  const installsOnly = projects.every((project) => project.mutation === 'install')
  if (!installsOnly) opts.strictPeerDependencies = false
  opts['forceNewModules'] = installsOnly
  const rootProjectManifest = projects.find(({ rootDir }) => rootDir === opts.lockfileDir)?.manifest ??
    // When running install/update on a subset of projects, the root project might not be included,
    // so reading its manifest explicitly here.
    await safeReadProjectManifestOnly(opts.lockfileDir)
  opts.hooks.readPackage = createReadPackageHook({
    readPackageHook: opts.hooks.readPackage,
    overrides: opts.overrides,
    lockfileDir: opts.lockfileDir,
    packageExtensions: opts.packageExtensions,
    peerDependencyRules: opts.peerDependencyRules,
  })
  const ctx = await getContext(projects, opts)
  const pruneVirtualStore = ctx.modulesFile?.prunedAt && opts.modulesCacheMaxAge > 0
    ? cacheExpired(ctx.modulesFile.prunedAt, opts.modulesCacheMaxAge)
    : true

  if (!maybeOpts.ignorePackageManifest) {
    for (const { manifest, rootDir } of ctx.projects) {
      if (!manifest) {
        throw new Error(`No package.json found in "${rootDir}"`)
      }
    }
  }

  const result = await _install()

  if (global['verifiedFileIntegrity'] > 1000) {
    globalWarn(`The integrity of ${global['verifiedFileIntegrity']} files was checked. This might have caused installation to take longer.`) // eslint-disable-line
  }
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  return result

  async function _install (): Promise<UpdatedProject[]> {
    const scriptsOpts: RunLifecycleHooksConcurrentlyOptions = {
      extraBinPaths: opts.extraBinPaths,
      rawConfig: opts.rawConfig,
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
    let needsFullResolution = !maybeOpts.ignorePackageManifest &&
      lockfileIsUpToDate(ctx.wantedLockfile, {
        overrides: opts.overrides,
        neverBuiltDependencies: opts.neverBuiltDependencies,
        onlyBuiltDependencies: opts.onlyBuiltDependencies,
        packageExtensionsChecksum,
      }) ||
      opts.fixLockfile
    if (needsFullResolution) {
      ctx.wantedLockfile.overrides = opts.overrides
      ctx.wantedLockfile.neverBuiltDependencies = opts.neverBuiltDependencies
      ctx.wantedLockfile.onlyBuiltDependencies = opts.onlyBuiltDependencies
      ctx.wantedLockfile.packageExtensionsChecksum = packageExtensionsChecksum
    }
    const frozenLockfile = opts.frozenLockfile ||
      opts.frozenLockfileIfExists && ctx.existsWantedLockfile
    if (
      !ctx.lockfileHadConflicts &&
      !opts.lockfileOnly &&
      !opts.update &&
      !opts.fixLockfile &&
      installsOnly &&
      (
        frozenLockfile ||
        opts.ignorePackageManifest ||
        !needsFullResolution &&
        opts.preferFrozenLockfile &&
        (!opts.pruneLockfileImporters || Object.keys(ctx.wantedLockfile.importers).length === ctx.projects.length) &&
        ctx.existsWantedLockfile &&
        ctx.wantedLockfile.lockfileVersion === LOCKFILE_VERSION &&
        await allProjectsAreUpToDate(ctx.projects, {
          linkWorkspacePackages: opts.linkWorkspacePackagesDepth >= 0,
          wantedLockfile: ctx.wantedLockfile,
          workspacePackages: opts.workspacePackages,
        })
      )
    ) {
      if (needsFullResolution) {
        throw new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE', 'Cannot perform a frozen installation because the lockfile needs updates', {
          hint: 'Note that in CI environments this setting is true by default. If you still need to run install in such cases, use "pnpm install --no-frozen-lockfile"',
        })
      }
      if (!ctx.existsWantedLockfile) {
        if (ctx.projects.some((project) => pkgHasDependencies(project.manifest))) {
          throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
        }
      } else {
        if (maybeOpts.ignorePackageManifest) {
          logger.info({ message: 'Importing packages to virtual store', prefix: opts.lockfileDir })
        } else {
          logger.info({ message: 'Lockfile is up-to-date, resolution step is skipped', prefix: opts.lockfileDir })
        }
        try {
          await headless({
            ...ctx,
            ...opts,
            currentEngine: {
              nodeVersion: opts.nodeVersion,
              pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
            },
            projects: ctx.projects as Project[],
            prunedAt: ctx.modulesFile?.prunedAt,
            pruneVirtualStore,
            wantedLockfile: maybeOpts.ignorePackageManifest ? undefined : ctx.wantedLockfile,
          })
          return projects
        } catch (error: any) { // eslint-disable-line
          if (
            frozenLockfile ||
            error.code !== 'ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY' && !BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code)
          ) throw error
          if (BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code)) {
            needsFullResolution = true
            // Ideally, we would not update but currently there is no other way to redownload the integrity of the package
            opts.update = true
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
    for (const project of ctx.projects) {
      switch (project.mutation) {
      case 'uninstallSome':
        projectsToInstall.push({
          pruneDirectDependencies: false,
          ...project,
          removePackages: project.dependencyNames,
          updatePackageManifest: true,
          wantedDependencies: [],
        })
        break
      case 'install': {
        await installCase({
          ...project,
          updatePackageManifest: opts.updatePackageManifest ?? opts.update,
        })
        break
      }
      case 'installSome': {
        await installSome({
          ...project,
          updatePackageManifest: opts.updatePackageManifest !== false,
        })
        break
      }
      case 'unlink': {
        const packageDirs = await readModulesDirs(project.modulesDir)
        const externalPackages = await pFilter(
          packageDirs!,
          async (packageDir: string) => isExternalLink(ctx.storeDir, project.modulesDir, packageDir)
        )
        const allDeps = getAllDependenciesFromManifest(project.manifest)
        const packagesToInstall: string[] = []
        for (const pkgName of externalPackages) {
          await rimraf(path.join(project.modulesDir, pkgName))
          if (allDeps[pkgName]) {
            packagesToInstall.push(pkgName)
          }
        }
        if (packagesToInstall.length === 0) return projects

        // TODO: install only those that were unlinked
        // but don't update their version specs in package.json
        await installCase({ ...project, mutation: 'install' })
        break
      }
      case 'unlinkSome': {
        if (project.manifest?.name && opts.globalBin) {
          await removeBin(path.join(opts.globalBin, project.manifest?.name))
        }
        const packagesToInstall: string[] = []
        const allDeps = getAllDependenciesFromManifest(project.manifest)
        for (const depName of project.dependencyNames) {
          try {
            if (!await isExternalLink(ctx.storeDir, project.modulesDir, depName)) {
              logger.warn({
                message: `${depName} is not an external link`,
                prefix: project.rootDir,
              })
              continue
            }
          } catch (err: any) { // eslint-disable-line
            if (err['code'] !== 'ENOENT') throw err // eslint-disable-line @typescript-eslint/dot-notation
          }
          await rimraf(path.join(project.modulesDir, depName))
          if (allDeps[depName]) {
            packagesToInstall.push(depName)
          }
        }
        if (packagesToInstall.length === 0) return projects

        // TODO: install only those that were unlinked
        // but don't update their version specs in package.json
        await installSome({
          ...project,
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
        includeDirect: opts.includeDirect,
        updateWorkspaceDependencies: opts.update,
        nodeExecPath: opts.nodeExecPath,
      })
        .map((wantedDependency) => ({ ...wantedDependency, updateSpec: true }))

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
        updateWorkspaceDependencies: opts.update,
        preferredSpecs,
        overrides: opts.overrides,
      })
      projectsToInstall.push({
        pruneDirectDependencies: false,
        ...project,
        wantedDependencies: wantedDeps.map(wantedDep => ({ ...wantedDep, isNew: true, updateSpec: true, nodeExecPath: opts.nodeExecPath })),
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
    })

    return result.projects
  }
}

function lockfileIsUpToDate (
  lockfile: Lockfile,
  {
    neverBuiltDependencies,
    onlyBuiltDependencies,
    overrides,
    packageExtensionsChecksum,
  }: {
    neverBuiltDependencies?: string[]
    onlyBuiltDependencies?: string[]
    overrides?: Record<string, string>
    packageExtensionsChecksum?: string
  }) {
  return !equals(lockfile.overrides ?? {}, overrides ?? {}) ||
    !equals((lockfile.neverBuiltDependencies ?? []).sort(), (neverBuiltDependencies ?? []).sort()) ||
    !equals(onlyBuiltDependencies?.sort(), lockfile.onlyBuiltDependencies) ||
    lockfile.packageExtensionsChecksum !== packageExtensionsChecksum
}

export function createObjectChecksum (obj: Object) {
  const s = JSON.stringify(obj)
  return crypto.createHash('md5').update(s).digest('hex')
}

export function createReadPackageHook (
  {
    lockfileDir,
    overrides,
    packageExtensions,
    peerDependencyRules,
    readPackageHook,
  }: {
    lockfileDir: string
    overrides?: Record<string, string>
    packageExtensions?: Record<string, PackageExtension>
    peerDependencyRules?: PeerDependencyRules
    readPackageHook?: ReadPackageHook
  }
): ReadPackageHook | undefined {
  const hooks: ReadPackageHook[] = []
  if (!isEmpty(overrides ?? {})) {
    hooks.push(createVersionsOverrider(overrides!, lockfileDir))
  }
  hooks.push(createPackageExtender(fromPairs(compatPackageExtensions)))
  if (!isEmpty(packageExtensions ?? {})) {
    hooks.push(createPackageExtender(packageExtensions!))
  }
  if (
    peerDependencyRules != null &&
    (!isEmpty(peerDependencyRules.ignoreMissing) || !isEmpty(peerDependencyRules.allowedVersions))
  ) {
    hooks.push(createPeerDependencyPatcher(peerDependencyRules))
  }
  if (hooks.length === 0) {
    return readPackageHook
  }
  const readPackageAndExtend = hooks.length === 1
    ? hooks[0]
    : pipeWith(async (f, res) => f(await res), hooks as any) as ReadPackageHook // eslint-disable-line @typescript-eslint/no-explicit-any
  if (readPackageHook != null) {
    return (async (manifest: ProjectManifest, dir?: string) => readPackageAndExtend(await readPackageHook(manifest, dir), dir)) as ReadPackageHook
  }
  return readPackageAndExtend
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

export async function addDependenciesToPackage (
  manifest: ProjectManifest,
  dependencySelectors: string[],
  opts: InstallOptions & {
    bin?: string
    allowNew?: boolean
    peer?: boolean
    pinnedVersion?: 'major' | 'minor' | 'patch'
    targetDependenciesField?: DependenciesField
  }
) {
  const projects = await mutateModules(
    [
      {
        binsDir: opts.bin,
        allowNew: opts.allowNew,
        dependencySelectors,
        manifest,
        mutation: 'installSome',
        peer: opts.peer,
        pinnedVersion: opts.pinnedVersion,
        rootDir: opts.dir ?? process.cwd(),
        targetDependenciesField: opts.targetDependenciesField,
      },
    ],
    {
      ...opts,
      lockfileDir: opts.lockfileDir ?? opts.dir,
    })
  return projects[0].manifest
}

export type ImporterToUpdate = {
  binsDir: string
  id: string
  manifest: ProjectManifest
  originalManifest?: ProjectManifest
  modulesDir: string
  rootDir: string
  pruneDirectDependencies: boolean
  removePackages?: string[]
  updatePackageManifest: boolean
  wantedDependencies: Array<WantedDependency & { isNew?: boolean, updateSpec?: boolean }>
} & DependenciesMutation

export interface UpdatedProject {
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
  ctx: PnpmContext<DependenciesMutation>,
  opts: StrictInstallOptions & {
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

  const preferredVersions = opts.preferredVersions ?? (
    (
      !opts.update &&
      (ctx.wantedLockfile.packages != null) &&
      !isEmpty(ctx.wantedLockfile.packages)
    )
      ? getPreferredVersionsFromLockfile(ctx.wantedLockfile.packages)
      : undefined
  )
  const forceFullResolution = ctx.wantedLockfile.lockfileVersion !== LOCKFILE_VERSION ||
    !opts.currentLockfileIsUpToDate ||
    opts.force ||
    opts.needsFullResolution ||
    ctx.lockfileHadConflicts

  // Ignore some fields when fixing lockfile, so these fields can be regenerated
  // and make sure it's up-to-date
  if (
    opts.fixLockfile &&
    (ctx.wantedLockfile.packages != null) &&
    !isEmpty(ctx.wantedLockfile.packages)
  ) {
    ctx.wantedLockfile.packages = Object.entries(ctx.wantedLockfile.packages).reduce((pre, [depPath, snapshot]) => ({
      ...pre,
      [depPath]: {
        // These fields are needed to avoid losing information of the locked dependencies if these fields are not broken
        // If these fields are broken, they will also be regenerated
        dependencies: snapshot.dependencies,
        optionalDependencies: snapshot.optionalDependencies,
        resolution: snapshot.resolution,
      },
    }), {})
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
      autoInstallPeers: opts.autoInstallPeers,
      currentLockfile: ctx.currentLockfile,
      defaultUpdateDepth: (opts.update || (opts.updateMatching != null)) ? opts.depth : -1,
      dryRun: opts.lockfileOnly,
      engineStrict: opts.engineStrict,
      force: opts.force,
      forceFullResolution,
      hooks: opts.hooks,
      linkWorkspacePackagesDepth: opts.linkWorkspacePackagesDepth ?? (opts.saveWorkspaceProtocol ? 0 : -1),
      lockfileDir: opts.lockfileDir,
      nodeVersion: opts.nodeVersion,
      pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
      preferWorkspacePackages: opts.preferWorkspacePackages,
      preferredVersions,
      preserveWorkspaceProtocol: opts.preserveWorkspaceProtocol,
      registries: ctx.registries,
      saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      storeController: opts.storeController,
      tag: opts.tag,
      updateMatching: opts.updateMatching,
      virtualStoreDir: ctx.virtualStoreDir,
      wantedLockfile: ctx.wantedLockfile,
      workspacePackages: opts.workspacePackages,
    }
  )

  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'resolution_done',
  })

  newLockfile = ((opts.hooks?.afterAllResolved) != null)
    ? await opts.hooks?.afterAllResolved(newLockfile)
    : newLockfile

  if (opts.updateLockfileMinorVersion) {
    newLockfile.lockfileVersion = LOCKFILE_VERSION
  }

  const depsStateCache: DepsStateCache = {}
  const lockfileOpts = { forceSharedFormat: opts.forceSharedLockfile }
  if (!opts.lockfileOnly && opts.enableModulesDir) {
    const result = await linkPackages(
      projects,
      dependenciesGraph,
      {
        currentLockfile: ctx.currentLockfile,
        dependenciesByProjectId,
        depsStateCache,
        extraNodePaths: ctx.extraNodePaths,
        force: opts.force,
        hoistedDependencies: ctx.hoistedDependencies,
        hoistedModulesDir: ctx.hoistedModulesDir,
        hoistPattern: ctx.hoistPattern,
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
      const importerNames = fromPairs(
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

    if (opts.ignoreScripts) {
      // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
      ctx.pendingBuilds = ctx.pendingBuilds
        .concat(
          result.newDepPaths
            .filter((depPath) => dependenciesGraph[depPath].requiresBuild)
        )
    } else if (result.newDepPaths?.length) {
      // postinstall hooks
      const depPaths = Object.keys(dependenciesGraph)
      const rootNodes = depPaths.filter((depPath) => dependenciesGraph[depPath].depth === 0)

      let extraEnv: Record<string, string> | undefined
      if (opts.enablePnp) {
        extraEnv = makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs'))
      }
      await buildModules(dependenciesGraph, rootNodes, {
        childConcurrency: opts.childConcurrency,
        depsStateCache,
        depsToBuild: new Set(result.newDepPaths),
        extraBinPaths: ctx.extraBinPaths,
        extraNodePaths: ctx.extraNodePaths,
        extraEnv,
        lockfileDir: ctx.lockfileDir,
        optional: opts.include.optionalDependencies,
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

    const binWarn = (prefix: string, message: string) => logger.info({ message, prefix })
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
          }, {})
        linkedPackages = await linkBins(project.modulesDir, project.binsDir, {
          allowExoticManifests: true,
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
          }
        )
      }
      const projectToInstall = projects[index]
      if (opts.global && projectToInstall.mutation.includes('install')) {
        projectToInstall.wantedDependencies.forEach(pkg => {
          if (!linkedPackages?.includes(pkg.alias)) {
            logger.warn({ message: `${pkg.alias} has no binaries`, prefix: opts.lockfileDir })
          }
        })
      }
    }))

    const projectsWithTargetDirs = extendProjectsWithTargetDirs(projects, newLockfile, ctx)
    await Promise.all([
      opts.useLockfile
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
        const injectedDeps = {}
        for (const project of projectsWithTargetDirs) {
          if (project.targetDirs.length > 0) {
            injectedDeps[project.id] = project.targetDirs.map((targetDir) => path.relative(opts.lockfileDir, targetDir))
          }
        }
        return writeModulesYaml(ctx.rootModulesDir, {
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
        })
      })(),
    ])
    if (!opts.ignoreScripts) {
      if (opts.enablePnp) {
        opts.scriptsOpts.extraEnv = makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs'))
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
    if (opts.useLockfile) {
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
      await headless({
        ...ctx,
        ...opts,
        currentEngine: {
          nodeVersion: opts.nodeVersion,
          pnpmVersion: opts.packageManager.name === 'pnpm' ? opts.packageManager.version : '',
        },
        projects: ctx.projects as Project[],
        prunedAt: ctx.modulesFile?.prunedAt,
        wantedLockfile: result.newLockfile,
      })
      return result
    }
    return await _installInContext(projects, ctx, opts)
  } catch (error: any) { // eslint-disable-line
    if (!BROKEN_LOCKFILE_INTEGRITY_ERRORS.has(error.code)) throw error
    opts.needsFullResolution = true
    // Ideally, we would not update but currently there is no other way to redownload the integrity of the package
    opts.update = true
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
    optional: boolean
    warn: (message: string) => void
  }
) {
  return unnest(await Promise.all(
    depNodes.map(async depNode => limitLinking(async () => linkBinsOfDependencies(depNode, depGraph, opts)))
  ))
}
