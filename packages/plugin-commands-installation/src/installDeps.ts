import path from 'path'
import {
  readProjectManifestOnly,
  tryReadProjectManifest,
} from '@pnpm/cli-utils'
import { Config } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import { filterPkgsBySelectorObjects } from '@pnpm/filter-workspace-packages'
import findWorkspacePackages, { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import { rebuild } from '@pnpm/plugin-commands-rebuild/lib/implementation'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { IncludedDependencies, Project } from '@pnpm/types'
import {
  install,
  mutateModules,
  WorkspacePackages,
} from '@pnpm/core'
import logger from '@pnpm/logger'
import { sequenceGraph } from '@pnpm/sort-packages'
import isSubdir from 'is-subdir'
import getPinnedVersion from './getPinnedVersion'
import getSaveType from './getSaveType'
import getNodeExecPath from './nodeExecPath'
import recursive, { createMatcher, matchDependencies } from './recursive'
import updateToLatestSpecsFromManifest, { createLatestSpecs } from './updateToLatestSpecsFromManifest'
import { createWorkspaceSpecs, updateToWorkspacePackagesFromManifest } from './updateWorkspaceDependencies'

const OVERWRITE_UPDATE_OPTIONS = {
  allowNew: true,
  update: false,
}

export type InstallDepsOptions = Pick<Config,
| 'allProjects'
| 'bail'
| 'bin'
| 'cliOptions'
| 'depth'
| 'dev'
| 'engineStrict'
| 'global'
| 'globalPnpmfile'
| 'hooks'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'linkWorkspacePackages'
| 'lockfileDir'
| 'lockfileOnly'
| 'pnpmfile'
| 'production'
| 'rawLocalConfig'
| 'registries'
| 'save'
| 'saveDev'
| 'saveExact'
| 'saveOptional'
| 'savePeer'
| 'savePrefix'
| 'saveProd'
| 'saveWorkspaceProtocol'
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
> & CreateStoreControllerOptions & {
  argv: {
    original: string[]
  }
  allowNew?: boolean
  frozenLockfileIfExists?: boolean
  include?: IncludedDependencies
  includeDirect?: IncludedDependencies
  latest?: boolean
  update?: boolean
  updateMatching?: (pkgName: string) => boolean
  updatePackageManifest?: boolean
  useBetaCli?: boolean
  recursive?: boolean
  workspace?: boolean
} & Partial<Pick<Config, 'pnpmHomeDir'>>

export default async function handler (
  opts: InstallDepsOptions,
  params: string[]
) {
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
    opts.workspaceDir ? await findWorkspacePackages(opts.workspaceDir, opts) : []
  )
  if (opts.workspaceDir) {
    const selectedProjectsGraph = opts.selectedProjectsGraph ?? selectProjectByDir(allProjects, opts.dir)
    if (selectedProjectsGraph != null) {
      const sequencedGraph = sequenceGraph(selectedProjectsGraph)
      // Check and warn if there are cyclic dependencies
      if (!sequencedGraph.safe) {
        const cyclicDependenciesInfo = sequencedGraph.cycles.length > 0
          ? `: ${sequencedGraph.cycles.map(deps => deps.join(', ')).join('; ')}`
          : ''
        logger.warn({
          message: `There are cyclic workspace dependencies${cyclicDependenciesInfo}`,
          prefix: opts.workspaceDir,
        })
      }

      await recursive(allProjects,
        params,
        {
          ...opts,
          forceHoistPattern,
          forcePublicHoistPattern,
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
    workspacePackages = arrayOfWorkspacePackagesToMap(allProjects)
  }

  const store = await createOrConnectStoreController(opts)
  const installOpts = {
    ...opts,
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

  let { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.dir, opts)
  if (manifest === null) {
    if (opts.update) {
      throw new PnpmError('NO_IMPORTER_MANIFEST', 'No package.json found')
    }
    manifest = {}
  }

  const updateMatch = opts.update && (params.length > 0) ? createMatcher(params) : null
  if (updateMatch != null) {
    params = matchDependencies(updateMatch, manifest, includeDirect)
    if (params.length === 0) {
      throw new PnpmError('NO_PACKAGE_IN_DEPENDENCIES',
        'None of the specified packages were found in the dependencies.')
    }
  }

  if (opts.update && opts.latest) {
    if (!params || (params.length === 0)) {
      params = updateToLatestSpecsFromManifest(manifest, includeDirect)
    } else {
      params = createLatestSpecs(params, manifest)
    }
  }
  if (opts.workspace) {
    if (!params || (params.length === 0)) {
      params = updateToWorkspacePackagesFromManifest(manifest, includeDirect, workspacePackages)
    } else {
      params = createWorkspaceSpecs(params, workspacePackages)
    }
  }
  if (params?.length) {
    const [updatedImporter] = await mutateModules([
      {
        allowNew: opts.allowNew,
        binsDir: installOpts.bin,
        dependencySelectors: params,
        manifest,
        mutation: 'installSome',
        peer: opts.savePeer,
        pinnedVersion: getPinnedVersion(opts),
        rootDir: installOpts.dir,
        targetDependenciesField: getSaveType(installOpts),
      },
    ], installOpts)
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
      selectedProjectsGraph,
      workspaceDir: opts.workspaceDir, // Otherwise TypeScript doesn't understant that is is not undefined
    }, 'install')

    if (opts.ignoreScripts) return

    await rebuild(
      [
        {
          buildIndex: 0,
          manifest: await readProjectManifestOnly(opts.dir, opts),
          rootDir: opts.dir,
        },
      ], {
        ...opts,
        pending: true,
        storeController: store.ctrl,
        storeDir: store.dir,
      }
    )
  }
}

function selectProjectByDir (projects: Project[], searchedDir: string) {
  const project = projects.find(({ dir }) => path.relative(dir, searchedDir) === '')
  if (project == null) return undefined
  return { [searchedDir]: { dependencies: [], package: project } }
}
