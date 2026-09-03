import { promises as fs } from 'node:fs'
import path from 'node:path'

import { mergeCatalogs } from '@pnpm/catalogs.config'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { CommandHandler } from '@pnpm/cli.command'
import {
  type RecursiveSummary,
  throwOnCommandFail,
} from '@pnpm/cli.utils'
import { createMatcherWithIndex } from '@pnpm/config.matcher'
import {
  type Config,
  type ConfigContext,
  createProjectConfigRecord,
  getWorkspaceConcurrency,
  type OptionsFromRootManifest,
  type ProjectConfig,
} from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { requireHooks } from '@pnpm/hooks.pnpmfile'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import {
  addDependenciesToPackage,
  type DryRunInstallResult,
  install,
  type InstallOptions,
  type MutatedProject,
  mutateModules,
  type ProjectOptions,
  type UpdateMatchingFunction,
  type WorkspacePackages,
} from '@pnpm/installing.deps-installer'
import { globalWarn, logger } from '@pnpm/logger'
import { filterDependenciesByType } from '@pnpm/pkg-manifest.utils'
import { getRangeSpecStyle } from '@pnpm/pkg-manifest.utils'
import type { PreferredVersions, ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type { StoreController } from '@pnpm/store.controller'
import type {
  DepPath,
  IgnoredBuilds,
  IncludedDependencies,
  PackageManifest,
  Project,
  ProjectManifest,
  ProjectRootDir,
  ProjectRootDirRealPath,
  ProjectsGraph,
  RangeSpecStyle,
} from '@pnpm/types'
import { filteredProjectsDependencies, projectsDependencies } from '@pnpm/workspace.projects-sorter'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'
import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'
import { isSubdir } from 'is-subdir'
import pFilter from 'p-filter'
import getVersionSelectorType from 'version-selector-type'

import { getSaveType } from './getSaveType.js'
import { handleIgnoredBuilds } from './handleIgnoredBuilds.js'
import { type PolicyViolation, setupPolicyHandlers } from './policyHandlers.js'
import { resolvedPackageVersionsForPrune } from './resolvedPackageVersionsForPrune.js'
import { toWorkspaceSpecs } from './updateWorkspaceDependencies.js'

export type RecursiveOptions = CreateStoreControllerOptions & Pick<Config,
| 'bail'
| 'configDependencies'
| 'dedupePeerDependents'
| 'dedupePeers'
| 'depth'
| 'dryRun'
| 'globalPnpmfile'
| 'hoistPattern'
| 'hoistingLimits'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'linkWorkspacePackages'
| 'lockfile'
| 'lockfileDir'
| 'lockfileOnly'
| 'modulesDir'
| 'pnprServer'
| 'allowBuilds'
| 'registriesByScope'
| 'runtime'
| 'save'
| 'saveCatalogName'
| 'saveDev'
| 'saveExact'
| 'saveOptional'
| 'savePeer'
| 'savePrefix'
| 'saveProd'
| 'saveWorkspaceProtocol'
| 'lockfileIncludeTarballUrl'
| 'sharedWorkspaceLockfile'
| 'tag'
| 'trustLockfile'
| 'tryLoadDefaultPnpmfile'
| 'catalogPrune'
| 'minimumReleaseAgeExcludePrune'
| 'packageConfigs'
| 'updateConfig'
> & Pick<ConfigContext,
| 'hooks'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
> & {
  rebuildHandler?: CommandHandler
  include?: IncludedDependencies
  includeDirect?: IncludedDependencies
  latest?: boolean
  pending?: boolean
  workspace?: boolean
  allowNew?: boolean
  ignoredPackages?: Set<string>
  update?: boolean
  updatePackageManifest?: boolean
  updateMatching?: UpdateMatchingFunction
  useBetaCli?: boolean
  allProjectsGraph: ProjectsGraph
  selectedProjectsGraph: ProjectsGraph
  prodAllProjectsGraph?: ProjectsGraph
  prodOnlySelectedProjectDirs?: ProjectRootDir[]
  preferredVersions?: PreferredVersions
  pruneDirectDependencies?: boolean
  pruneLockfileImporters?: boolean
  storeControllerAndDir?: {
    ctrl: StoreController
    dir: string
    resolutionVerifiers: ResolutionVerifier[]
  }
  pnpmfile: string[]
  /**
   * Alternative install engine (today: pacquet) the deps-installer
   * delegates the install to. Built in `installDeps` when
   * `configDependencies.pacquet` is declared, threaded through here so
   * the recursive workspace path picks it up too.
   */
  runPacquet?: {
    supportsResolution: boolean
    run: (opts?: { filterResolvedProgress?: boolean, resolve?: boolean }) => Promise<void>
  }
} & Partial<
  Pick<Config,
| 'ci'
| 'sort'
| 'strictDepBuilds'
| 'workspaceConcurrency'
  >
> & Required<
  Pick<Config, 'workspaceDir'>
>

export type CommandFullName = 'install' | 'add' | 'remove' | 'update' | 'import'

export interface RecursiveResult {
  passed: boolean | string
  /**
   * Catalog entries written to `pnpm-workspace.yaml` during this install.
   * The caller folds these into the catalogs recorded in the workspace state
   * cache so that reverting a catalog entry is detected as an outdated state.
   */
  updatedCatalogs?: Catalogs
  /**
   * Present only for a `dryRun` install over a shared workspace lockfile:
   * the before/after wanted lockfiles for the caller to diff.
   */
  dryRunResult?: DryRunInstallResult
}

export async function recursive (
  allProjects: Project[],
  params: string[],
  opts: RecursiveOptions,
  cmdFullName: CommandFullName
): Promise<RecursiveResult> {
  if (allProjects.length === 0) {
    // It might make sense to throw an exception in this case
    return { passed: false }
  }

  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)

  if (pkgs.length === 0) {
    return { passed: false }
  }
  const manifestsByPath = getManifestsByPath(allProjects)

  const throwOnFail = throwOnCommandFail.bind(null, `pnpm recursive ${cmdFullName}`)

  const store = opts.storeControllerAndDir ?? await createStoreController(opts)

  const workspacePackages: WorkspacePackages = arrayOfWorkspacePackagesToMap(allProjects) as WorkspacePackages
  const targetDependenciesField = getSaveType(opts)
  // See `installDeps.ts` for context; mirrored here so workspace-recursive
  // installs also surface immature picks (loose-mode auto-persist or
  // strict-mode prompt). The workspace manifest writer dedupes against the
  // existing list, so a single drain at the end captures additions across
  // every project.
  const policyHandlers = setupPolicyHandlers(opts)
  const projectDependencies = opts.sort !== false
    ? projectsDependencies(opts.allProjectsGraph)
    : new Map((Object.keys(opts.allProjectsGraph) as ProjectRootDir[]).sort().map((rootDir) => [rootDir, []]))
  const installOpts = Object.assign(opts, {
    allProjects: getAllProjects(manifestsByPath, opts.allProjectsGraph),
    linkWorkspacePackagesDepth: opts.linkWorkspacePackages === 'deep' ? Infinity : opts.linkWorkspacePackages ? 0 : -1,
    ownLifecycleHooksStdio: 'pipe',
    peer: opts.savePeer,
    pruneLockfileImporters: opts.pruneLockfileImporters ??
      (((opts.ignoredPackages == null) || opts.ignoredPackages.size === 0) &&
        pkgs.length === allProjects.length),
    saveCatalogName: opts.saveCatalogName,
    skipRuntimes: opts.runtime === false,
    storeController: store.ctrl,
    storeDir: store.dir,
    targetDependenciesField,
    resolutionVerifiers: store.resolutionVerifiers,
    projectDependencies,
    workspacePackages,
    handleResolutionPolicyViolations: policyHandlers?.handleResolutionPolicyViolations,
  }) as InstallOptions

  const result: RecursiveSummary = {}

  const projectConfigRecord = createProjectConfigRecord(opts)
  const getProjectConfig: (manifest: Pick<ProjectManifest, 'name'>) => ProjectConfig | undefined =
    projectConfigRecord
      ? manifest => manifest.name ? projectConfigRecord[manifest.name] : undefined
      : () => undefined

  const updateToLatest = opts.update && opts.latest
  const includeDirect = opts.includeDirect ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }

  let updateMatch: UpdateDepsMatcher | null
  // `params` is rewritten per project into the dependency names it matched, so
  // remember whether the user named any package. `--workspace` only insists
  // that a dependency exists in the workspace when it was asked for by name.
  const userNamedDeps = params.length > 0
  if (cmdFullName === 'update') {
    if (params.length === 0) {
      const ignoreDeps = opts.updateConfig?.ignoreDependencies
      if (ignoreDeps?.length) {
        params = makeIgnorePatterns(ignoreDeps)
      }
    }
    updateMatch = params.length ? createMatcher(params) : null
  } else {
    updateMatch = null
  }
  // At `--depth 0` a selector that matches no direct dependency is already
  // `NO_PACKAGE_IN_DEPENDENCIES` below; only a deeper update reaches the
  // transitive copy whose version cannot be recorded. `--latest` rejects every
  // versioned selector on its own, direct or not, and has to report that
  // first.
  if (updateMatch != null && !opts.latest && (opts.depth ?? Infinity) > 0) {
    failOnVersionsOfIndirectUpdateSpecs(params, pkgs.map(({ manifest }) => manifest), includeDirect)
  }
  // For a workspace with shared lockfile
  if (opts.lockfileDir && ['add', 'install', 'remove', 'update', 'import'].includes(cmdFullName)) {
    let importers = getImporters(opts)
    const calculatedRepositoryRoot = await fs.realpath(calculateRepositoryRoot(opts.workspaceDir, importers.map(x => x.rootDir)))
    const isFromWorkspace = isSubdir.bind(null, calculatedRepositoryRoot)
    importers = await pFilter(importers, async ({ rootDirRealPath }) => isFromWorkspace(rootDirRealPath))
    if (importers.length === 0) return { passed: true }
    let mutation: 'install' | 'installSome' | 'uninstallSome'
    switch (cmdFullName) {
      case 'remove':
        mutation = 'uninstallSome'
        break
      case 'import':
        mutation = 'install'
        break
      default:
        mutation = (params.length === 0 && !updateToLatest ? 'install' : 'installSome')
        break
    }
    const mutatedImporters = [] as MutatedProject[]
    await Promise.all(importers.map(async ({ rootDir }) => {
      const { manifest } = manifestsByPath[rootDir]
      const localConfig = getProjectConfig(manifest) ?? {}
      const modulesDir = localConfig.modulesDir ?? opts.modulesDir
      let currentInput = [...params]
      if (updateMatch != null) {
        currentInput = matchDependencies(updateMatch, manifest, includeDirect)
        if ((currentInput.length === 0) && (typeof opts.depth === 'undefined' || opts.depth <= 0)) {
          installOpts.pruneLockfileImporters = false
          return
        }
      }
      if (updateToLatest && (!params || (params.length === 0))) {
        currentInput = Object.keys(filterDependenciesByType(manifest, includeDirect))
      }
      if (opts.workspace) {
        currentInput = toWorkspaceSpecs(currentInput, {
          manifest,
          include: includeDirect,
          workspacePackages,
          userNamedDeps,
        })
      }
      switch (mutation) {
        case 'uninstallSome':
          mutatedImporters.push({
            dependencyNames: currentInput,
            modulesDir,
            mutation,
            rootDir,
            targetDependenciesField,
          } as MutatedProject)
          return
        case 'installSome':
          mutatedImporters.push({
            allowNew: cmdFullName === 'install' || cmdFullName === 'add',
            dependencySelectors: currentInput,
            modulesDir,
            mutation,
            peer: opts.savePeer,
            rangeSpecStyle: getRangeSpecStyle({
              saveExact: typeof localConfig.saveExact === 'boolean' ? localConfig.saveExact : opts.saveExact,
              savePrefix: typeof localConfig.savePrefix === 'string' ? localConfig.savePrefix : opts.savePrefix,
            }),
            rootDir,
            targetDependenciesField,
            update: opts.update,
            updateMatching: opts.updateMatching,
            updatePackageManifest: opts.updatePackageManifest,
            updateToLatest: opts.latest,
          } as MutatedProject)
          return
        case 'install':
          mutatedImporters.push({
            modulesDir,
            mutation,
            pruneDirectDependencies: opts.pruneDirectDependencies,
            rootDir,
            update: opts.update,
            updateMatching: opts.updateMatching,
            updatePackageManifest: opts.updatePackageManifest,
            updateToLatest: opts.latest,
          } as MutatedProject)
      }
    }))
    if (!opts.selectedProjectsGraph[opts.workspaceDir as ProjectRootDir] && manifestsByPath[opts.workspaceDir as ProjectRootDir] != null) {
      mutatedImporters.push({
        mutation: 'install',
        rootDir: opts.workspaceDir as ProjectRootDir,
      })
    }
    if ((mutatedImporters.length === 0) && cmdFullName === 'update' && opts.depth === 0) {
      throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
        'None of the specified packages were found in the dependencies of any of the projects.')
    }
    const {
      updatedCatalogs,
      updatedProjects: mutatedPkgs,
      ignoredBuilds,
      newLockfile,
      resolutionPolicyViolations,
      dryRunResult,
    } = await mutateModules(mutatedImporters, {
      ...installOpts,
      storeController: store.ctrl,
      resolutionVerifiers: store.resolutionVerifiers,
    })
    if (opts.save !== false && !opts.dryRun) {
      // Only pick entries when we'll actually persist. Otherwise the
      // info log would claim entries were added that the workspace
      // manifest never saw, and the next install would re-prompt or
      // fail verification.
      const policyUpdates = policyHandlers?.pickManifestUpdates(resolutionPolicyViolations)
      const promises: Array<Promise<void>> = mutatedPkgs.map(async ({ originalManifest, manifest, rootDir }) => {
        return manifestsByPath[rootDir].writeProjectManifest(originalManifest ?? manifest)
      })
      promises.push(updateWorkspaceManifest(opts.workspaceDir, {
        updatedCatalogs,
        catalogPrune: opts.catalogPrune,
        resolvedPackageVersions: resolvedPackageVersionsForPrune(opts, newLockfile),
        minimumReleaseAgeExcludePrune: opts.minimumReleaseAgeExcludePrune,
        allProjects,
        ...policyUpdates,
      }))
      await Promise.all(promises)
    }
    await handleIgnoredBuilds(opts, ignoredBuilds)
    return { passed: true, updatedCatalogs, dryRunResult }
  }

  const pkgPaths = (Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]).sort()
  const selectedProjectDependencies = opts.sort !== false
    ? filteredProjectsDependencies(opts)
    : new Map(pkgPaths.map((rootDir) => [rootDir, []]))

  let updatedCatalogs: Catalogs | undefined

  const allIgnoredBuilds = new Set<DepPath>()
  // Each per-project install returns its own slice of lockfile-resolution
  // violations; accumulate them here so the post-loop persist step can
  // dedup and write a single batch to the workspace manifest.
  const allResolutionPolicyViolations: PolicyViolation[] = []
  let firstError: Error | undefined
  await scheduleGraph(selectedProjectDependencies, {
    bail: opts.bail !== false,
    concurrency: getWorkspaceConcurrency(opts.workspaceConcurrency),
    continueOnFailure: opts.bail === false,
    runNode: async (rootDir): Promise<TaskCompletion> => {
      try {
        if (opts.ignoredPackages?.has(rootDir)) {
          return 'passed'
        }
        result[rootDir] = { status: 'running' }
        const hooks = opts.ignorePnpmfile
          ? {}
          : await (async () => {
            const { hooks: pnpmfileHooks } = await requireHooks(rootDir, opts)
            return {
              ...opts.hooks,
              ...pnpmfileHooks,
              afterAllResolved: [...(pnpmfileHooks.afterAllResolved ?? []), ...(opts.hooks?.afterAllResolved ?? [])],
              readPackage: [...(pnpmfileHooks.readPackage ?? []), ...(opts.hooks?.readPackage ?? [])],
            }
          })()
        const { manifest, writeProjectManifest } = manifestsByPath[rootDir]
        let currentInput = [...params]
        if (updateMatch != null) {
          currentInput = matchDependencies(updateMatch, manifest, includeDirect)
          if (currentInput.length === 0) return 'passed'
        }
        if (updateToLatest && (!params || (params.length === 0))) {
          currentInput = Object.keys(filterDependenciesByType(manifest, includeDirect))
        }
        if (opts.workspace) {
          currentInput = toWorkspaceSpecs(currentInput, {
            manifest,
            include: includeDirect,
            workspacePackages,
            userNamedDeps,
          })
        }

        type ActionOpts =
          & Omit<InstallOptions, 'allProjects'>
          & OptionsFromRootManifest
          & Project
          & Pick<Config, 'bin'>
          & { rangeSpecStyle: RangeSpecStyle }

        interface ActionResult {
          updatedCatalogs?: Catalogs
          updatedManifest: ProjectManifest
          ignoredBuilds: IgnoredBuilds | undefined
          resolutionPolicyViolations?: PolicyViolation[]
        }

        type ActionFunction = (manifest: PackageManifest | ProjectManifest, opts: ActionOpts) => Promise<ActionResult>

        let action: ActionFunction
        switch (cmdFullName) {
          case 'remove':
            action = async (manifest, opts) => {
              const mutationResult = await mutateModules([
                {
                  dependencyNames: currentInput,
                  mutation: 'uninstallSome',
                  rootDir,
                },
              ], opts)
              return {
                updatedCatalogs: undefined, // there's no reason to add new or update catalogs on `pnpm remove`
                updatedManifest: mutationResult.updatedProjects[0].manifest,
                ignoredBuilds: mutationResult.ignoredBuilds,
                resolutionPolicyViolations: mutationResult.resolutionPolicyViolations,
              }
            }
            break
          default:
            action = currentInput.length === 0
              ? install
              : async (manifest, opts) => addDependenciesToPackage(manifest, currentInput, opts)
            break
        }

        const localConfig = getProjectConfig(manifest) ?? {}
        const {
          updatedCatalogs: newCatalogsAddition,
          updatedManifest: newManifest,
          ignoredBuilds,
          resolutionPolicyViolations,
        } = await action(
          manifest,
          {
            ...installOpts,
            ...localConfig,
            ...opts.allProjectsGraph[rootDir]?.package,
            bin: path.join(rootDir, 'node_modules', '.bin'),
            dir: rootDir,
            hooks,
            ignoreScripts: true,
            rangeSpecStyle: getRangeSpecStyle({
              saveExact: typeof localConfig.saveExact === 'boolean' ? localConfig.saveExact : opts.saveExact,
              savePrefix: typeof localConfig.savePrefix === 'string' ? localConfig.savePrefix : opts.savePrefix,
            }),
            configByUri: installOpts.configByUri,
            storeController: store.ctrl,
            resolutionVerifiers: store.resolutionVerifiers,
          }
        )
        if (opts.save !== false) {
          await writeProjectManifest(newManifest)
          if (newCatalogsAddition) {
            // Per-project additions are partial maps keyed by catalog name then
            // dependency. Merge at the dependency level so two projects updating
            // different entries of the same catalog don't clobber each other.
            updatedCatalogs = mergeCatalogs(updatedCatalogs, newCatalogsAddition)
          }
        }
        if (ignoredBuilds?.size) {
          for (const depPath of ignoredBuilds) {
            allIgnoredBuilds.add(depPath)
          }
        }
        if (resolutionPolicyViolations?.length) {
          for (const violation of resolutionPolicyViolations) {
            allResolutionPolicyViolations.push(violation)
          }
        }
        result[rootDir].status = 'passed'
        return 'passed'
      } catch (err: any) { // eslint-disable-line
        logger.info(err)

        if (!opts.bail) {
          result[rootDir] = {
            status: 'failure',
            error: err,
            message: err.message,
            prefix: rootDir,
          }
          return 'failed'
        }

        err['prefix'] = rootDir
        firstError ??= err
        return 'aborted'
      }
    },
    onNodeSkipped: () => {},
  })
  if (firstError != null) throw firstError
  await handleIgnoredBuilds(opts, allIgnoredBuilds.size ? allIgnoredBuilds : undefined)
  if (opts.save !== false) {
    // Only pick entries when we'll actually persist. Otherwise the
    // info log would claim entries were added that the workspace
    // manifest never saw, mirroring the gate the shared-lockfile
    // branch + installDeps already apply.
    await updateWorkspaceManifest(opts.workspaceDir, {
      updatedCatalogs,
      catalogPrune: opts.catalogPrune,
      allProjects,
      ...policyHandlers?.pickManifestUpdates(allResolutionPolicyViolations),
    })
  }

  if (
    !opts.lockfileOnly && !opts.ignoreScripts && (
      cmdFullName === 'add' ||
      cmdFullName === 'install' ||
      cmdFullName === 'update'
    )
  ) {
    await opts.rebuildHandler?.({
      ...opts,
      pending: opts.pending === true,
      skipIfHasSideEffectsCache: true,
    }, [])
  }

  throwOnFail(result)

  if (!Object.values(result).filter(({ status }) => status === 'passed').length && cmdFullName === 'update' && opts.depth === 0) {
    throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
      'None of the specified packages were found in the dependencies of any of the projects.')
  }

  return { passed: true, updatedCatalogs }
}

function calculateRepositoryRoot (
  workspaceDir: string,
  projectDirs: string[]
): string {
  // assume repo root is workspace dir
  let relativeRepoRoot = '.'
  for (const rootDir of projectDirs) {
    const relativePartRegExp = new RegExp(`^(\\.\\.\\${path.sep})+`)
    const relativePartMatch = relativePartRegExp.exec(path.relative(workspaceDir, rootDir))
    if (relativePartMatch != null) {
      const relativePart = relativePartMatch[0]
      if (relativePart.length > relativeRepoRoot.length) {
        relativeRepoRoot = relativePart
      }
    }
  }
  return path.resolve(workspaceDir, relativeRepoRoot)
}

export function matchDependencies (
  match: (input: string) => string | null,
  manifest: ProjectManifest,
  include: IncludedDependencies
): string[] {
  const deps = Object.keys(filterDependenciesByType(manifest, include))
  const matchedDeps = []
  for (const dep of deps) {
    const spec = match(dep)
    if (spec === null) continue
    matchedDeps.push(spec ? `${dep}@${spec}` : dep)
  }
  return matchedDeps
}

/**
 * The update-target predicate of `pnpm update <selector>...`. The version part
 * of an exact selector narrows which resolved copies of a matched package are
 * update targets: `foo@1.2.3` targets only the version line that can resolve to
 * `1.2.3` — the same major, or the same minor when the request is on `0.x`,
 * where the minor is the compatibility boundary. A package the workspace
 * depends on twice therefore keeps the copies on its other lines untouched.
 *
 * A selector that carries a range, a tag, or no version at all targets by name
 * alone, and so does every call made before the edge's resolved version is
 * known. Negated selectors exclude names, never versions.
 */
export function createUpdateMatching (params: string[]): UpdateMatchingFunction {
  const parsed = params.map(parseUpdateParam)
  const matchesAnySelector = createMatcherWithIndex(parsed.map(({ pattern }) => pattern))
  const versionScopes = parsed
    .filter(({ pattern }) => pattern[0] !== '!')
    .map(({ pattern, versionSpec }) => ({
      matchesPattern: createMatcherWithIndex([pattern]),
      requestedLine: versionSpec != null ? parseVersionLine(versionSpec) : undefined,
    }))
  return (pkgName: string, version?: string) => {
    if (matchesAnySelector(pkgName) === -1) return false
    if (versionScopes.length === 0) return true
    for (const { matchesPattern, requestedLine } of versionScopes) {
      if (matchesPattern(pkgName) === -1) continue
      if (requestedLine == null || version == null) return true
      const currentLine = parseVersionLine(version)
      if (currentLine == null || currentLine.major !== requestedLine.major) continue
      if (requestedLine.major !== 0 || currentLine.minor === requestedLine.minor) return true
    }
    return false
  }
}

/**
 * The version a selector names, normalized, or `undefined` for a range, a tag
 * or an `npm:` alias spec — none of which name a single version.
 */
function parseExactVersion (versionSpec: string): string | undefined {
  const selector = getVersionSelectorType(versionSpec)
  return selector?.type === 'version' ? selector.normalized : undefined
}

/** The major and minor of the version a selector names, if it names one. */
function parseVersionLine (versionSpec: string): { major: number, minor: number } | undefined {
  const version = parseExactVersion(versionSpec)
  if (version == null) return undefined
  const [major, minor] = version.split('.')
  return { major: Number(major), minor: Number(minor) }
}

/**
 * `pnpm update <dep>@<version>` where `<dep>` matches no direct dependency has
 * nowhere to record the version. An update resolves such a target the same way
 * a fresh install would — which a command-line version cannot influence — so
 * honoring the request would mean writing a lockfile entry no manifest backs,
 * and the next fresh resolve would undo it. Neither npm nor Yarn accepts a
 * version here either. Fail rather than resolve to something else and leave
 * the caller a zero exit status to read.
 *
 * A range or a tag is not held to the same standard: it names no single
 * version to record, and updating within the dependents' ranges is a
 * reasonable reading of it. Those keep the warning they have always had.
 *
 * The override the hint recommends is scoped to the dependents' declared range
 * so it cannot violate any consumer's range; that range lives in the
 * dependents' manifests, which this layer does not read, hence the
 * placeholder.
 */
export function failOnVersionsOfIndirectUpdateSpecs (
  updateSpecs: string[],
  manifests: ProjectManifest[],
  include: IncludedDependencies
): void {
  const pinned: Array<{ pattern: string, version: string }> = []
  for (const spec of updateSpecs) {
    const { pattern, versionSpec } = parseUpdateParam(spec)
    // A negated selector excludes names; a version on one asks for nothing.
    if (versionSpec == null || pattern[0] === '!') continue
    if (matchesADirectDependency(pattern, manifests, include)) continue
    const version = parseExactVersion(versionSpec)
    if (version == null) {
      globalWarn(`"${pattern}" is not a direct dependency, so the requested "${versionSpec}" is ignored — "${pattern}" is updated to what a fresh install would resolve.`)
      continue
    }
    pinned.push({ pattern, version })
  }
  if (pinned.length === 0) return
  const subjects = pinned.map(({ pattern, version }) => `"${pattern}" (requested "${version}")`)
  const overrides = pinned.map(({ pattern, version }) => `    ${pattern}@<declared range>: ${version}`)
  throw new PnpmError('UPDATE_VERSION_ON_INDIRECT_DEP',
    `${subjects.join(', ')} ${pinned.length === 1 ? 'is not a direct dependency, so the requested version cannot' : 'are not direct dependencies, so the requested versions cannot'} be recorded.`,
    {
      hint: `An update resolves a transitive dependency the way a fresh install would, so a version on the command line has no effect on it. To pin one, add an override scoped to the range its dependents declare to pnpm-workspace.yaml:

  overrides:
${overrides.join('\n')}

To update it within the range its dependents already declare, drop the version: pnpm update ${pinned.map(({ pattern }) => pattern).join(' ')}`,
    })
}

/**
 * Whether any of `manifests` declares a dependency `pattern` names, so the
 * update has a manifest entry to write the requested version into. A pattern
 * that matches nothing directly reaches its target only through the resolver,
 * which the version cannot steer.
 */
function matchesADirectDependency (
  pattern: string,
  manifests: ProjectManifest[],
  include: IncludedDependencies
): boolean {
  const match = createMatcher([pattern])
  return manifests.some((manifest) => matchDependencies(match, manifest, include).length > 0)
}

export type UpdateDepsMatcher = (input: string) => string | null

export function createMatcher (params: string[]): UpdateDepsMatcher {
  const patterns: string[] = []
  const specs: string[] = []
  for (const param of params) {
    const { pattern, versionSpec } = parseUpdateParam(param)
    patterns.push(pattern)
    specs.push(versionSpec ?? '')
  }
  const matcher = createMatcherWithIndex(patterns)
  return (depName: string) => {
    const index = matcher(depName)
    if (index === -1) return null
    return specs[index]
  }
}

export function parseUpdateParam (param: string): { pattern: string, versionSpec: string | undefined } {
  const atIndex = param.indexOf('@', param[0] === '!' ? 2 : 1)
  if (atIndex === -1) {
    return {
      pattern: param,
      versionSpec: undefined,
    }
  }
  return {
    pattern: param.slice(0, atIndex),
    versionSpec: param.slice(atIndex + 1),
  }
}

/**
 * The selectors an update selector stands for. An `npm:` selector contributes
 * a second one for the aliased package, because that — not the alias — is the
 * name the resolver resolves the edge under; it carries the aliased spec's own
 * version so the expansion scopes the same version line the user asked for.
 */
export function expandUpdateSelectorsForMatching (selector: string): string[] {
  const { pattern, versionSpec } = parseUpdateParam(selector)
  if (versionSpec?.startsWith('npm:') !== true) return [selector]
  const aliasSelector = parseUpdateParam(versionSpec.slice('npm:'.length))
  const aliasPattern = pattern[0] === '!' ? `!${aliasSelector.pattern}` : aliasSelector.pattern
  const aliasSpec = aliasSelector.versionSpec != null ? `${aliasPattern}@${aliasSelector.versionSpec}` : aliasPattern
  return [selector, aliasSpec]
}

export function makeIgnorePatterns (ignoredDependencies: string[]): string[] {
  return ignoredDependencies.map(depName => `!${depName}`)
}

function getAllProjects (manifestsByPath: ManifestsByPath, allProjectsGraph: ProjectsGraph): ProjectOptions[] {
  return (Object.keys(allProjectsGraph) as ProjectRootDir[]).map((rootDir) => {
    const { rootDirRealPath, modulesDir } = allProjectsGraph[rootDir].package
    return {
      buildIndex: 0,
      manifest: manifestsByPath[rootDir].manifest,
      rootDir,
      rootDirRealPath,
      modulesDir,
    }
  })
}

interface ManifestsByPath {
  [dir: string]: Omit<Project, 'rootDir' | 'rootDirRealPath'>
}

function getManifestsByPath (projects: Project[]): Record<ProjectRootDir, Omit<Project, 'rootDir' | 'rootDirRealPath'>> {
  const manifestsByPath: Record<string, Omit<Project, 'rootDir' | 'rootDirRealPath'>> = {}
  for (const { rootDir, manifest, writeProjectManifest } of projects) {
    manifestsByPath[rootDir] = { manifest, writeProjectManifest }
  }
  return manifestsByPath
}

function getImporters (opts: Pick<RecursiveOptions, 'selectedProjectsGraph' | 'ignoredPackages'>): Array<{ rootDir: ProjectRootDir, rootDirRealPath: ProjectRootDirRealPath }> {
  let rootDirs = Object.keys(opts.selectedProjectsGraph) as ProjectRootDir[]
  if (opts.ignoredPackages != null) {
    rootDirs = rootDirs.filter((rootDir) => !opts.ignoredPackages!.has(rootDir))
  }
  return rootDirs.map((rootDir) => ({ rootDir, rootDirRealPath: opts.selectedProjectsGraph[rootDir].package.rootDirRealPath }))
}
