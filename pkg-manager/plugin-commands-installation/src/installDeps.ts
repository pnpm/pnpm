import path from 'path'
import {
  readProjectManifestOnly,
  tryReadProjectManifest,
} from '@pnpm/cli-utils'
import { type Config, getOptionsFromRootManifest } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/get-context'
import { filterPkgsBySelectorObjects } from '@pnpm/filter-workspace-packages'
import { filterDependenciesByType } from '@pnpm/manifest-utils'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { type Lockfile } from '@pnpm/lockfile-types'
import { rebuildProjects } from '@pnpm/plugin-commands-rebuild'
import { createOrConnectStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { type IncludedDependencies, type Project, type ProjectsGraph, type ProjectRootDir } from '@pnpm/types'
import {
  install,
  mutateModulesInSingleProject,
  type MutateModulesOptions,
  type WorkspacePackages,
} from '@pnpm/core'
import { logger } from '@pnpm/logger'
import { sequenceGraph } from '@pnpm/sort-packages'
import { createPkgGraph } from '@pnpm/workspace.pkgs-graph'
import isSubdir from 'is-subdir'
import { getPinnedVersion } from './getPinnedVersion'
import { getSaveType } from './getSaveType'
import { getNodeExecPath } from './nodeExecPath'
import { recursive, createMatcher, matchDependencies, makeIgnorePatterns, type UpdateDepsMatcher } from './recursive'
import { createWorkspaceSpecs, updateToWorkspacePackagesFromManifest } from './updateWorkspaceDependencies'

const OVERWRITE_UPDATE_OPTIONS = {
  allowNew: true,
  update: false,
}

export type InstallDepsOptions = Pick<Config,
| 'allProjects'
| 'allProjectsGraph'
| 'autoInstallPeers'
| 'bail'
| 'bin'
| 'cliOptions'
| 'dedupePeerDependents'
| 'depth'
| 'dev'
| 'engineStrict'
| 'global'
| 'globalPnpmfile'
| 'hooks'
| 'ignoreCurrentPrefs'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'linkWorkspacePackages'
| 'lockfileDir'
| 'lockfileOnly'
| 'pnpmfile'
| 'production'
| 'rawLocalConfig'
| 'registries'
| 'rootProjectManifestDir'
| 'rootProjectManifest'
| 'save'
| 'saveDev'
| 'saveExact'
| 'saveOptional'
| 'savePeer'
| 'savePrefix'
| 'saveProd'
| 'saveWorkspaceProtocol'
| 'lockfileIncludeTarballUrl'
| 'scriptsPrependNodePath'
| 'scriptShell'
| 'selectedProjectsGraph'
| 'sideEffectsCache'
| 'sideEffectsCacheReadonly'
| 'sort'
| 'sharedWorkspaceLockfile'
| 'shellEmulator'
| 'tag'
| 'optional'
| 'workspaceConcurrency'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'extraEnv'
| 'ignoreWorkspaceCycles'
| 'disallowWorkspaceCycles'
> & CreateStoreControllerOptions & {
  argv: {
    original: string[]
  }
  allowNew?: boolean
  forceFullResolution?: boolean
  frozenLockfileIfExists?: boolean
  include?: IncludedDependencies
  includeDirect?: IncludedDependencies
  latest?: boolean
  /**
   * If specified, the installation will only be performed for comparison of the
   * wanted lockfile. The wanted lockfile will not be updated on disk and no
   * modules will be linked.
   *
   * The given callback is passed the wanted lockfile before installation and
   * after. This allows functions to reasonably determine whether the wanted
   * lockfile will change on disk after installation. The lockfile arguments
   * passed to this callback should not be mutated.
   */
  lockfileCheck?: (prev: Lockfile, next: Lockfile) => void
  update?: boolean
  updateToLatest?: boolean
  updateMatching?: (pkgName: string) => boolean
  updatePackageManifest?: boolean
  useBetaCli?: boolean
  recursive?: boolean
  dedupe?: boolean
  workspace?: boolean
  includeOnlyPackageFiles?: boolean
} & Partial<Pick<Config, 'pnpmHomeDir'>>

export async function installDeps (
  opts: InstallDepsOptions,
  params: string[]
): Promise<void> {
  if (opts.workspace) {
    if (opts.latest) {
      throw new PnpmError('BAD_OPTIONS', 'Cannot use --latest with --workspace simultaneously')
    }
    if (!opts.workspaceDir) {
      throw new PnpmError('WORKSPACE_OPTION_OUTSIDE_WORKSPACE', '--workspace can only be used inside a workspace')
    }
    if (!opts.linkWorkspacePackages && !opts.saveWorkspaceProtocol) {
      if (opts.rawLocalConfig['save-workspace-protocol'] === false) {
        throw new PnpmError('BAD_OPTIONS', 'This workspace has link-workspace-packages turned off, \
so dependencies are linked from the workspace only when the workspace protocol is used. \
Either set link-workspace-packages to true or don\'t use the --no-save-workspace-protocol option \
when running add/update with the --workspace option')
      } else {
        opts.saveWorkspaceProtocol = true
      }
    }
    // @ts-expect-error
    opts['preserveWorkspaceProtocol'] = !opts.linkWorkspacePackages
  }
  const includeDirect = opts.includeDirect ?? {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }
  const forceHoistPattern = typeof opts.rawLocalConfig['hoist-pattern'] !== 'undefined' ||
    typeof opts.rawLocalConfig['hoist'] !== 'undefined'
  const forcePublicHoistPattern = typeof opts.rawLocalConfig['shamefully-hoist'] !== 'undefined' ||
    typeof opts.rawLocalConfig['public-hoist-pattern'] !== 'undefined'
  const allProjects = opts.allProjects ?? (
    opts.workspaceDir
      ? await findWorkspacePackages(opts.workspaceDir, { ...opts, patterns: opts.workspacePackagePatterns })
      : []
  )
  if (opts.workspaceDir) {
    const selectedProjectsGraph = opts.selectedProjectsGraph ?? selectProjectByDir(allProjects, opts.dir)
    if (selectedProjectsGraph != null) {
      const sequencedGraph = sequenceGraph(selectedProjectsGraph)
      // Check and warn if there are cyclic dependencies
      if (!opts.ignoreWorkspaceCycles && !sequencedGraph.safe) {
        const cyclicDependenciesInfo = sequencedGraph.cycles.length > 0
          ? `: ${sequencedGraph.cycles.map(deps => deps.join(', ')).join('; ')}`
          : ''

        if (opts.disallowWorkspaceCycles) {
          throw new PnpmError('DISALLOW_WORKSPACE_CYCLES', `There are cyclic workspace dependencies${cyclicDependenciesInfo}`)
        }

        logger.warn({
          message: `There are cyclic workspace dependencies${cyclicDependenciesInfo}`,
          prefix: opts.workspaceDir,
        })
      }

      let allProjectsGraph!: ProjectsGraph
      if (opts.dedupePeerDependents) {
        allProjectsGraph = opts.allProjectsGraph ?? createPkgGraph(allProjects, {
          linkWorkspacePackages: Boolean(opts.linkWorkspacePackages),
        }).graph
      } else {
        allProjectsGraph = selectedProjectsGraph
        if (!allProjectsGraph[opts.workspaceDir as ProjectRootDir]) {
          allProjectsGraph = {
            ...allProjectsGraph,
            ...selectProjectByDir(allProjects, opts.workspaceDir),
          }
        }
      }
      await recursive(allProjects,
        params,
        {
          ...opts,
          ...getOptionsFromRootManifest(opts.rootProjectManifestDir, opts.rootProjectManifest ?? {}),
          forceHoistPattern,
          forcePublicHoistPattern,
          allProjectsGraph,
          selectedProjectsGraph,
          workspaceDir: opts.workspaceDir,
        },
        opts.update ? 'update' : (params.length === 0 ? 'install' : 'add')
      )
      return
    }
  }
  // `pnpm install ""` is going to be just `pnpm install`
  params = params.filter(Boolean)

  const dir = opts.dir || process.cwd()
  let workspacePackages!: WorkspacePackages

  if (opts.workspaceDir) {
    workspacePackages = arrayOfWorkspacePackagesToMap(allProjects) as WorkspacePackages
  }

  let { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.dir, opts)
  if (manifest === null) {
    if (opts.update === true || params.length === 0) {
      throw new PnpmError('NO_PKG_MANIFEST', `No package.json found in ${opts.dir}`)
    }
    manifest = {}
  }

  const store = await createOrConnectStoreController(opts)
  const installOpts: Omit<MutateModulesOptions, 'allProjects'> = {
    ...opts,
    ...getOptionsFromRootManifest(opts.dir, manifest),
    forceHoistPattern,
    forcePublicHoistPattern,
    // In case installation is done in a multi-package repository
    // The dependencies should be built first,
    // so ignoring scripts for now
    ignoreScripts: !!workspacePackages || opts.ignoreScripts,
    linkWorkspacePackagesDepth: opts.linkWorkspacePackages === 'deep' ? Infinity : opts.linkWorkspacePackages ? 0 : -1,
    sideEffectsCacheRead: opts.sideEffectsCache ?? opts.sideEffectsCacheReadonly,
    sideEffectsCacheWrite: opts.sideEffectsCache,
    storeController: store.ctrl,
    storeDir: store.dir,
    workspacePackages,
  }
  if (opts.global && opts.pnpmHomeDir != null) {
    const nodeExecPath = await getNodeExecPath()
    if (isSubdir(opts.pnpmHomeDir, nodeExecPath)) {
      installOpts['nodeExecPath'] = nodeExecPath
    }
  }

  let updateMatch: UpdateDepsMatcher | null
  if (opts.update) {
    if (params.length === 0) {
      const ignoreDeps = manifest.pnpm?.updateConfig?.ignoreDependencies
      if (ignoreDeps?.length) {
        params = makeIgnorePatterns(ignoreDeps)
      }
    }
    updateMatch = params.length ? createMatcher(params) : null
  } else {
    updateMatch = null
  }
  if (updateMatch != null) {
    params = matchDependencies(updateMatch, manifest, includeDirect)
    if (params.length === 0) {
      if (opts.latest) return
      if (opts.depth === 0) {
        throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
          'None of the specified packages were found in the dependencies.')
      }
    }
  }

  if (opts.update && opts.latest && (!params || (params.length === 0))) {
    params = Object.keys(filterDependenciesByType(manifest, includeDirect))
  }
  if (opts.workspace) {
    if (!params || (params.length === 0)) {
      params = updateToWorkspacePackagesFromManifest(manifest, includeDirect, workspacePackages)
    } else {
      params = createWorkspaceSpecs(params, workspacePackages)
    }
  }
  if (params?.length) {
    const mutatedProject = {
      allowNew: opts.allowNew,
      binsDir: opts.bin,
      dependencySelectors: params,
      manifest,
      mutation: 'installSome' as const,
      peer: opts.savePeer,
      pinnedVersion: getPinnedVersion(opts),
      rootDir: opts.dir as ProjectRootDir,
      targetDependenciesField: getSaveType(opts),
    }
    const updatedImporter = await mutateModulesInSingleProject(mutatedProject, installOpts)
    if (opts.save !== false) {
      await writeProjectManifest(updatedImporter.manifest)
    }
    return
  }

  const updatedManifest = await install(manifest, installOpts)
  if (opts.update === true && opts.save !== false) {
    await writeProjectManifest(updatedManifest)
  }

  if (opts.linkWorkspacePackages && opts.workspaceDir) {
    const { selectedProjectsGraph } = await filterPkgsBySelectorObjects(allProjects, [
      {
        excludeSelf: true,
        includeDependencies: true,
        parentDir: dir,
      },
    ], {
      workspaceDir: opts.workspaceDir,
    })
    await recursive(allProjects, [], {
      ...opts,
      ...OVERWRITE_UPDATE_OPTIONS,
      allProjectsGraph: opts.allProjectsGraph!,
      selectedProjectsGraph,
      workspaceDir: opts.workspaceDir, // Otherwise TypeScript doesn't understand that is not undefined
    }, 'install')

    if (opts.ignoreScripts) return

    await rebuildProjects(
      [
        {
          buildIndex: 0,
          manifest: await readProjectManifestOnly(opts.dir, opts),
          rootDir: opts.dir as ProjectRootDir,
        },
      ], {
        ...opts,
        pending: true,
        storeController: store.ctrl,
        storeDir: store.dir,
        skipIfHasSideEffectsCache: true,
      }
    )
  }
}

function selectProjectByDir (projects: Project[], searchedDir: string): ProjectsGraph | undefined {
  const project = projects.find(({ rootDir }) => path.relative(rootDir, searchedDir) === '')
  if (project == null) return undefined
  return { [searchedDir]: { dependencies: [], package: project } }
}
