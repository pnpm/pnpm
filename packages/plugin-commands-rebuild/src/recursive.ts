import {
  RecursiveSummary,
  throwOnCommandFail,
} from '@pnpm/cli-utils'
import {
  Config,
  readLocalConfig,
} from '@pnpm/config'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import { logger } from '@pnpm/logger'
import { sortPackages } from '@pnpm/sort-packages'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { Project, ProjectManifest } from '@pnpm/types'
import mem from 'mem'
import pLimit from 'p-limit'
import { rebuildProjects as rebuildAll, RebuildOptions, rebuildSelectedPkgs } from './implementation'

type RecursiveRebuildOpts = CreateStoreControllerOptions & Pick<Config,
| 'hoistPattern'
| 'hooks'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'lockfileDir'
| 'lockfileOnly'
| 'rawLocalConfig'
| 'registries'
| 'sharedWorkspaceLockfile'
> & {
  pending?: boolean
} & Partial<Pick<Config, 'bail' | 'sort' | 'workspaceConcurrency'>>

export async function recursiveRebuild (
  allProjects: Project[],
  params: string[],
  opts: RecursiveRebuildOpts & {
    ignoredPackages?: Set<string>
  } & Required<Pick<Config, 'selectedProjectsGraph' | 'workspaceDir'>>
) {
  if (allProjects.length === 0) {
    // It might make sense to throw an exception in this case
    return
  }

  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)

  if (pkgs.length === 0) {
    return
  }
  const manifestsByPath: { [dir: string]: Omit<Project, 'dir'> } = {}
  for (const { dir, manifest, writeProjectManifest } of pkgs) {
    manifestsByPath[dir] = { manifest, writeProjectManifest }
  }

  const throwOnFail = throwOnCommandFail.bind(null, 'pnpm recursive rebuild')

  const chunks = opts.sort !== false
    ? sortPackages(opts.selectedProjectsGraph)
    : [Object.keys(opts.selectedProjectsGraph).sort()]

  const store = await createOrConnectStoreController(opts)

  const workspacePackages = arrayOfWorkspacePackagesToMap(allProjects)
  const rebuildOpts = Object.assign(opts, {
    ownLifecycleHooksStdio: 'pipe',
    pruneLockfileImporters: ((opts.ignoredPackages == null) || opts.ignoredPackages.size === 0) &&
      pkgs.length === allProjects.length,
    storeController: store.ctrl,
    storeDir: store.dir,
    workspacePackages,
  }) as RebuildOptions

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  const memReadLocalConfig = mem(readLocalConfig)

  async function getImporters () {
    const importers = [] as Array<{ buildIndex: number, manifest: ProjectManifest, rootDir: string }>
    await Promise.all(chunks.map(async (prefixes: string[], buildIndex) => {
      if (opts.ignoredPackages != null) {
        prefixes = prefixes.filter((prefix) => !opts.ignoredPackages!.has(prefix))
      }
      return Promise.all(
        prefixes.map(async (prefix) => {
          importers.push({
            buildIndex,
            manifest: manifestsByPath[prefix].manifest,
            rootDir: prefix,
          })
        })
      )
    }))
    return importers
  }

  const rebuild = (
    params.length === 0
      ? rebuildAll
    : (importers: any, opts: any) => rebuildSelectedPkgs(importers, params, opts) // eslint-disable-line
  )
  if (opts.lockfileDir) {
    const importers = await getImporters()
    await rebuild(
      importers,
      {
        ...rebuildOpts,
        pending: opts.pending === true,
      }
    )
    return
  }
  const limitRebuild = pLimit(opts.workspaceConcurrency ?? 4)
  for (const chunk of chunks) {
    await Promise.all(chunk.map(async (rootDir: string) =>
      limitRebuild(async () => {
        try {
          if (opts.ignoredPackages?.has(rootDir)) {
            return
          }
          const localConfig = await memReadLocalConfig(rootDir)
          await rebuild(
            [
              {
                buildIndex: 0,
                manifest: manifestsByPath[rootDir].manifest,
                rootDir,
              },
            ],
            {
              ...rebuildOpts,
              ...localConfig,
              dir: rootDir,
              pending: opts.pending === true,
              rawConfig: {
                ...rebuildOpts.rawConfig,
                ...localConfig,
              },
            }
          )
          result.passes++
        } catch (err: any) { // eslint-disable-line
          logger.info(err)

          if (!opts.bail) {
            result.fails.push({
              error: err,
              message: err.message,
              prefix: rootDir,
            })
            return
          }

          err['prefix'] = rootDir
          throw err
        }
      })
    ))
  }

  throwOnFail(result)
}
