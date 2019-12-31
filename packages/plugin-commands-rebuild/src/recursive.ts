import {
  RecursiveSummary,
  throwOnCommandFail,
} from '@pnpm/cli-utils'
import { Config, types as allTypes, Project, ProjectsGraph } from '@pnpm/config'
import { scopeLogger } from '@pnpm/core-loggers'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import logger from '@pnpm/logger'
import sortPackages from '@pnpm/sort-packages'
import { createOrConnectStoreController, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { ProjectManifest, PackageManifest } from '@pnpm/types'
import camelcaseKeys = require('camelcase-keys')
import mem = require('mem')
import pLimit from 'p-limit'
import path = require('path')
import readIniFile = require('read-ini-file')
import { rebuild as rebuildAll, RebuildOptions, rebuildPkgs } from './implementation'

type RecursiveRebuildOpts = CreateStoreControllerOptions & Pick<Config,
  'hoistPattern' |
  'ignorePnpmfile' |
  'ignoreScripts' |
  'lockfileDir' |
  'lockfileOnly' |
  'rawLocalConfig' |
  'registries' |
  'sharedWorkspaceLockfile'
> & {
  pending?: boolean,
} & Partial<Pick<Config, 'bail' | 'sort' | 'workspaceConcurrency'>>

export default async function recursive (
  allProjects: Project[],
  input: string[],
  opts: RecursiveRebuildOpts & {
    ignoredPackages?: Set<string>,
  } & Required<Pick<Config, 'selectedProjectsGraph' | 'workspaceDir'>>,
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

  scopeLogger.debug({
    selected: pkgs.length,
    total: allProjects.length,
    workspacePrefix: opts.workspaceDir,
  })

  const throwOnFail = throwOnCommandFail.bind(null, `pnpm recursive rebuild`)

  const chunks = opts.sort !== false
    ? sortPackages(opts.selectedProjectsGraph)
    : [Object.keys(opts.selectedProjectsGraph).sort()]

  const store = await createOrConnectStoreController(opts)

  const workspacePackages = arrayOfWorkspacePackagesToMap(allProjects)
  const rebuildOpts = Object.assign(opts, {
    ownLifecycleHooksStdio: 'pipe',
    pruneLockfileImporters: (!opts.ignoredPackages || opts.ignoredPackages.size === 0)
      && pkgs.length === allProjects.length,
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
    await Promise.all(chunks.map((prefixes: string[], buildIndex) => {
      if (opts.ignoredPackages) {
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
    input.length === 0
    ? rebuildAll
    : (importers: any, opts: any) => rebuildPkgs(importers, input, opts) // tslint:disable-line
  )
  if (opts.lockfileDir) {
    const importers = await getImporters()
    await rebuild(
      importers,
      {
        ...rebuildOpts,
        pending: opts.pending === true,
      },
    )
    return
  }
  const limitRebuild = pLimit(opts.workspaceConcurrency ?? 4)
  for (const chunk of chunks) {
    await Promise.all(chunk.map((rootDir: string) =>
      limitRebuild(async () => {
        try {
          if (opts.ignoredPackages && opts.ignoredPackages.has(rootDir)) {
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
            },
          )
          result.passes++
        } catch (err) {
          logger.info(err)

          if (!opts.bail) {
            result.fails.push({
              error: err,
              message: err.message,
              prefix: rootDir,
            })
            return
          }

          err['prefix'] = rootDir // tslint:disable-line:no-string-literal
          throw err
        }
      }),
    ))
  }

  throwOnFail(result)
}

async function readLocalConfig (prefix: string) {
  try {
    const ini = await readIniFile(path.join(prefix, '.npmrc')) as { [key: string]: string }
    const config = camelcaseKeys(ini) as ({ [key: string]: string } & { hoist?: boolean })
    if (config.shamefullyFlatten) {
      config.hoistPattern = '*'
      // TODO: print a warning
    }
    if (config.hoist === false) {
      config.hoistPattern = ''
    }
    return config
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
    return {}
  }
}
