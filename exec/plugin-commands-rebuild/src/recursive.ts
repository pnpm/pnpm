import mem from 'mem'
import pLimit from 'p-limit'

import { logger } from '@pnpm/logger'
import {
  createOrConnectStoreController,
  type CreateStoreControllerOptions,
} from '@pnpm/store-connection-manager'
import { sortPackages } from '@pnpm/sort-packages'
import { type Config, readLocalConfig } from '@pnpm/config'
import { type RecursiveSummary, throwOnCommandFail } from '@pnpm/cli-utils'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/workspace.find-packages'
import type { Project, ProjectManifest, RebuildOptions } from '@pnpm/types'

import {
  rebuildSelectedPkgs,
  rebuildProjects as rebuildAll,
} from './implementation'

type RecursiveRebuildOpts = CreateStoreControllerOptions &
  Pick<
    Config,
    | 'hoistPattern'
    | 'hooks'
    | 'ignorePnpmfile'
    | 'ignoreScripts'
    | 'lockfileDir'
    | 'lockfileOnly'
    | 'nodeLinker'
    | 'rawLocalConfig'
    | 'registries'
    | 'rootProjectManifest'
    | 'rootProjectManifestDir'
    | 'sharedWorkspaceLockfile'
  > & {
    pending?: boolean
  } & Partial<Pick<Config, 'bail' | 'sort' | 'workspaceConcurrency'>>

export async function recursiveRebuild(
  allProjects: Project[],
  params: string[],
  opts: RecursiveRebuildOpts & {
    ignoredPackages?: Set<string>
  } & Required<Pick<Config, 'selectedProjectsGraph' | 'workspaceDir'>>
): Promise<void> {
  if (allProjects.length === 0) {
    // It might make sense to throw an exception in this case
    return
  }

  const pkgs = Object.values(opts.selectedProjectsGraph).map(
    (wsPkg) => wsPkg.package
  )

  if (pkgs.length === 0) {
    return
  }
  const manifestsByPath: { [dir: string]: Omit<Project, 'dir' | 'rootDir' | 'modulesDir' | 'id'> } = {}

  for (const { dir, manifest, writeProjectManifest } of pkgs) {
    manifestsByPath[dir] = { manifest, writeProjectManifest }
  }

  const throwOnFail = throwOnCommandFail.bind(null, 'pnpm recursive rebuild')

  const chunks =
    opts.sort !== false
      ? sortPackages(opts.selectedProjectsGraph)
      : [Object.keys(opts.selectedProjectsGraph).sort()]

  const store = await createOrConnectStoreController(opts)

  const workspacePackages = arrayOfWorkspacePackagesToMap(allProjects)
  const rebuildOpts = Object.assign(opts, {
    ownLifecycleHooksStdio: 'pipe',
    pruneLockfileImporters:
      (opts.ignoredPackages == null || opts.ignoredPackages.size === 0) &&
      pkgs.length === allProjects.length,
    storeController: store.ctrl,
    storeDir: store.dir,
    workspacePackages,
  }) as RebuildOptions

  const result: RecursiveSummary = {}

  const memReadLocalConfig = mem(readLocalConfig)

  async function getImporters() {
    const importers: Array<{
      buildIndex: number
      manifest: ProjectManifest | undefined
      rootDir: string
    }> = []

    await Promise.all(
      chunks.map(async (prefixes: string[], buildIndex: number): Promise<void[]> => {
        if (opts.ignoredPackages != null) {
          prefixes = prefixes.filter(
            (prefix: string): boolean => {
              return !opts.ignoredPackages?.has(prefix);
            }
          )
        }

        return Promise.all(
          prefixes.map(async (prefix: string): Promise<void> => {
            importers.push({
              buildIndex,
              manifest: manifestsByPath[prefix].manifest,
              rootDir: prefix,
            })
          })
        )
      })
    )

    return importers
  }

  const rebuild =
    params.length === 0
      ? rebuildAll
      : (importers: {
        buildIndex: number;
        manifest: ProjectManifest | undefined;
        rootDir: string;
      }[], opts: RebuildOptions): Promise<void> => {
        return rebuildSelectedPkgs(importers, params, opts) // eslint-disable-line;
      }

  if (opts.lockfileDir) {
    const importers = await getImporters()

    await rebuild(importers, {
      ...rebuildOpts,
      pending: opts.pending === true,
    })
    return
  }
  const limitRebuild = pLimit(opts.workspaceConcurrency ?? 4)
  for (const chunk of chunks) {
    // eslint-disable-next-line no-await-in-loop
    await Promise.all(
      chunk.map(async (rootDir: string) => {
        return limitRebuild(async () => {
          try {
            if (opts.ignoredPackages?.has(rootDir)) {
              return
            }

            result[rootDir] = { status: 'running' }

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
            result[rootDir].status = 'passed'
          } catch (err: any) { // eslint-disable-line
            logger.info(err)

            if (!opts.bail) {
              result[rootDir] = {
                status: 'failure',
                error: err,
                message: err.message,
                prefix: rootDir,
              }
              return
            }

            err.prefix = rootDir
            throw err
          }
        });
      }
      )
    )
  }

  throwOnFail(result)
}
