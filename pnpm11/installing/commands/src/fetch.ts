import path from 'node:path'

import { UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, createProjectConfigRecord, getWorkspaceConcurrency } from '@pnpm/config.reader'
import { type InstallOptions, mutateModulesInSingleProject } from '@pnpm/installing.deps-installer'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type { ProjectRootDir } from '@pnpm/types'
import { scheduleGraph } from '@pnpm/workspace.task-scheduler'
import { renderHelp } from 'render-help'

import { cliOptionsTypes } from './install.js'

export const rcOptionsTypes = cliOptionsTypes

export { cliOptionsTypes }

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['fetch']

export function help (): string {
  return renderHelp({
    description: 'Fetch packages from a lockfile into virtual store, package manifest is ignored. WARNING! This is an experimental command. Breaking changes may be introduced in non-major versions of the CLI',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Only development packages will be fetched',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Development packages will not be fetched',
            name: '--prod',
            shortAlias: '-P',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('fetch'),
    usages: ['pnpm fetch [--dev | --prod]'],
  })
}

type FetchCommandOptions = Pick<Config, 'production' | 'dev' | 'optional' | 'allowBuilds' | 'enableGlobalVirtualStore' | 'nodeLinker' | 'patchedDependencies' | 'recursive' | 'sharedWorkspaceLockfile' | 'workspaceConcurrency' | 'bail' | 'packageConfigs' | 'lockfileDir'> & Pick<ConfigContext, 'rootProjectManifest' | 'rootProjectManifestDir' | 'selectedProjectsGraph'> & CreateStoreControllerOptions

export async function handler (opts: FetchCommandOptions): Promise<void> {
  const store = await createStoreController(opts)
  if (opts.recursive && opts.sharedWorkspaceLockfile === false && opts.selectedProjectsGraph) {
    await fetchProjects(opts, store)
    return
  }
  await fetchProject(opts, store, path.resolve(opts.dir ?? process.cwd()) as ProjectRootDir)
}

async function fetchProjects (
  opts: FetchCommandOptions,
  store: Awaited<ReturnType<typeof createStoreController>>
): Promise<void> {
  const projectConfigs = createProjectConfigRecord(opts)
  const projects = Object.values(opts.selectedProjectsGraph!).map(({ package: project }) => project)
  let firstError: unknown
  await scheduleGraph(new Map(projects.map(project => [project, []])), {
    bail: opts.bail !== false,
    concurrency: getWorkspaceConcurrency(opts.workspaceConcurrency),
    continueOnFailure: opts.bail === false,
    onNodeSkipped: () => {},
    runNode: async (project) => {
      try {
        await fetchProject({
          ...opts,
          ...(project.manifest.name ? projectConfigs?.[project.manifest.name] : undefined),
          dir: project.rootDir,
          lockfileDir: project.rootDir,
        }, store, project.rootDir)
        return 'passed'
      } catch (error: unknown) {
        firstError ??= error
        return 'failed'
      }
    },
  })
  if (firstError) throw firstError
}

async function fetchProject (
  opts: FetchCommandOptions,
  store: Awaited<ReturnType<typeof createStoreController>>,
  rootDir: ProjectRootDir
): Promise<void> {
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  await mutateModulesInSingleProject({
    manifest: {},
    mutation: 'install',
    pruneDirectDependencies: true,
    rootDir,
  }, {
    ...opts,
    ignorePackageManifest: true,
    ignoreLocalPackages: true,
    include,
    modulesCacheMaxAge: 0,
    pruneStore: true,
    storeController: store.ctrl,
    storeDir: store.dir,
    resolutionVerifiers: store.resolutionVerifiers,
    // Hoisting is skipped anyway,
    // so we store these empty patterns in node_modules/.modules.yaml
    // to let the subsequent install know that hoisting should be performed.
    hoistPattern: [],
    publicHoistPattern: [],
    // virtualStoreOnly skips post-import linking (symlinks, bins, hoisting)
    // even if ignorePackageManifest handling changes in the future.
    virtualStoreOnly: true,
    // Ensure fetch can populate the virtual store even when the user has
    // enable-modules-dir=false in their config — fetch always needs node_modules/.pnpm
    // (unless GVS is active, in which case enableModulesDir doesn't matter).
    enableModulesDir: true,
  } as InstallOptions)
}
