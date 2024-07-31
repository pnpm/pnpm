import assert from 'assert'
import util from 'util'
import {
  type RecursiveSummary,
  throwOnCommandFail,
} from '@pnpm/cli-utils'
import {
  type Config,
  readLocalConfig,
} from '@pnpm/config'
import { logger } from '@pnpm/logger'
import { sortPackages } from '@pnpm/sort-packages'
import { createOrConnectStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { type Project, type ProjectManifest, type ProjectRootDir } from '@pnpm/types'
import mem from 'mem'
import pLimit from 'p-limit'
import { rebuildProjects as rebuildAll, type RebuildOptions, rebuildSelectedPkgs } from './implementation'

type RecursiveRebuildOpts = CreateStoreControllerOptions & Pick<Config,
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

export async function recursiveRebuild (
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

  const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)

  if (pkgs.length === 0) {
    return
  }
  const manifestsByPath: { [dir: string]: Omit<Project, 'rootDir' | 'rootDirRealPath'> } = {}
  for (const { rootDir, manifest, writeProjectManifest } of pkgs) {
    manifestsByPath[rootDir] = { manifest, writeProjectManifest }
  }

  const throwOnFail = throwOnCommandFail.bind(null, 'pnpm recursive rebuild')

  const chunks = opts.sort !== false
    ? sortPackages(opts.selectedProjectsGraph)
    : [Object.keys(opts.selectedProjectsGraph).sort() as ProjectRootDir[]]

  const store = await createOrConnectStoreController(opts)

  const rebuildOpts = Object.assign(opts, {
    ownLifecycleHooksStdio: 'pipe',
    pruneLockfileImporters: ((opts.ignoredPackages == null) || opts.ignoredPackages.size === 0) &&
      pkgs.length === allProjects.length,
    storeController: store.ctrl,
    storeDir: store.dir,
  }) as RebuildOptions

  const result: RecursiveSummary = {}

  const memReadLocalConfig = mem(readLocalConfig)

  async function getImporters () {
    const importers = [] as Array<{ buildIndex: number, manifest: ProjectManifest, rootDir: ProjectRootDir }>
    await Promise.all(chunks.map(async (prefixes, buildIndex) => {
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
    // eslint-disable-next-line no-await-in-loop
    await Promise.all(chunk.map(async (rootDir) =>
      limitRebuild(async () => {
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
        } catch (err: unknown) {
          assert(util.types.isNativeError(err))
          const errWithPrefix = Object.assign(err, {
            prefix: rootDir,
          })
          logger.info(errWithPrefix)

          if (!opts.bail) {
            result[rootDir] = {
              status: 'failure',
              error: errWithPrefix,
              message: err.message,
              prefix: rootDir,
            }
            return
          }

          throw err
        }
      })
    ))
  }

  throwOnFail(result)
}
