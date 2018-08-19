import logger from '@pnpm/logger'
import {PackageJson} from '@pnpm/types'
import camelcaseKeys = require('camelcase-keys')
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
import { StoreController } from 'package-store'
import path = require('path')
import createPkgGraph from 'pkgs-graph'
import readIniFile = require('read-ini-file')
import {
  install,
  InstallOptions,
  installPkgs,
  rebuild,
  rebuildPkgs,
  uninstall,
  unlink,
  unlinkPkgs,
} from 'supi'
import createStoreController from '../../createStoreController'
import findWorkspacePackages, {arrayOfLocalPackagesToMap} from '../../findWorkspacePackages'
import getCommandFullName from '../../getCommandFullName'
import requireHooks from '../../requireHooks'
import {PnpmOptions} from '../../types'
import help from '../help'
import exec from './exec'
import {
  filterGraph,
  filterGraphByEntryDirectory,
  filterGraphByScope,
} from './filter'
import list from './list'
import outdated from './outdated'
import RecursiveSummary, {throwOnCommandFail} from './recursiveSummary'
import run from './run'

const supportedRecursiveCommands = new Set([
  'install',
  'uninstall',
  'update',
  'link',
  'unlink',
  'list',
  'outdated',
  'rebuild',
  'run',
  'test',
  'exec',
])

export default async (
  input: string[],
  opts: PnpmOptions,
) => {
  if (opts.workspaceConcurrency < 1) {
    const err = new Error('Workspace concurrency should be at least 1')
    err['code'] = 'ERR_PNPM_INVALID_WORKSPACE_CONCURRENCY' // tslint:disable-line:no-string-literal
    throw err
  }

  const cmd = input.shift()
  if (!cmd) {
    help(['recursive'])
    return
  }
  const cmdFullName = getCommandFullName(cmd)
  if (!supportedRecursiveCommands.has(cmdFullName)) {
    help(['recursive'])
    const err = new Error(`"recursive ${cmdFullName}" is not a pnpm command. See "pnpm help recursive".`)
    err['code'] = 'ERR_PNPM_INVALID_RECURSIVE_COMMAND' // tslint:disable-line:no-string-literal
    throw err
  }

  const cwd = process.cwd()
  const allWorkspacePkgs = await findWorkspacePackages(cwd)
  return recursive(allWorkspacePkgs, input, opts, cmdFullName, cmd)
}

export async function recursive (
  allPkgs: Array<{path: string, manifest: PackageJson}>,
  input: string[],
  opts: PnpmOptions & {
    filterByEntryDirectory?: string,
    inputForEntryDirectory?: string[],
  },
  cmdFullName: string,
  cmd: string,
) {
  const pkgGraphResult = createPkgGraph(allPkgs)
  let pkgs: Array<{path: string, manifest: PackageJson}>
  if (opts.scope) {
    pkgGraphResult.graph = filterGraphByScope(pkgGraphResult.graph, opts.scope)
    pkgs = allPkgs.filter((pkg: {path: string}) => pkgGraphResult.graph[pkg.path])
  } else if (opts.filter) {
    pkgGraphResult.graph = filterGraph(pkgGraphResult.graph, opts.filter)
    pkgs = allPkgs.filter((pkg: {path: string}) => pkgGraphResult.graph[pkg.path])
  } else if (opts.filterByEntryDirectory) {
    pkgGraphResult.graph = filterGraphByEntryDirectory(pkgGraphResult.graph, opts.filterByEntryDirectory)
    pkgs = allPkgs.filter((pkg: {path: string}) => pkgGraphResult.graph[pkg.path])
  } else {
    pkgs = allPkgs
  }

  const throwOnFail = throwOnCommandFail.bind(null, `pnpm recursive ${cmd}`)

  switch (cmdFullName) {
    case 'list':
      await list(pkgs, input, cmd, opts as any) // tslint:disable-line:no-any
      return
    case 'outdated':
      await outdated(pkgs, input, cmd, opts as any) // tslint:disable-line:no-any
      return
    case 'test':
      throwOnFail(await run(pkgs, ['test', ...input], cmd, opts as any)) // tslint:disable-line:no-any
      return
    case 'run':
      throwOnFail(await run(pkgs, input, cmd, opts as any)) // tslint:disable-line:no-any
      return
    case 'update':
      opts = {...opts, update: true, allowNew: false} as any // tslint:disable-line:no-any
      break
    case 'exec':
      throwOnFail(await exec(pkgs, input, cmd, opts as any)) // tslint:disable-line:no-any
      return
  }

  const store = await createStoreController(opts)

  // It is enough to save the store.json file once,
  // once all installations are done.
  // That's why saveState that is passed to the install engine
  // does nothing.
  const saveState = store.ctrl.saveState
  const storeController = {
    ...store.ctrl,
    saveState: async () => undefined,
  }

  const graph = new Map(
    Object.keys(pkgGraphResult.graph).map((pkgPath) => [pkgPath, pkgGraphResult.graph[pkgPath].dependencies]) as Array<[string, string[]]>,
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [Object.keys(pkgGraphResult.graph)],
  })
  const chunks = graphSequencerResult.chunks

  if (cmdFullName === 'link' && opts.linkWorkspacePackages) {
    const err = new Error('"pnpm recursive link" is deprecated with link-workspace-packages = true. Please use "pnpm recursive install" instead')
    err['code'] = 'ERR_PNPM_RECURSIVE_LINK_DEPRECATED' // tslint:disable-line:no-string-literal
    throw err
  }
  const localPackages = cmdFullName === 'link' || opts.linkWorkspacePackages
    ? arrayOfLocalPackagesToMap(allPkgs)
    : {}
  const installOpts = Object.assign(opts, {
    localPackages,
    ownLifecycleHooksStdio: 'pipe',
    store: store.path,
    storeController,
  }) as InstallOptions

  const limitInstallation = pLimit(opts.workspaceConcurrency)
  let action!: any // tslint:disable-line:no-any
  switch (cmdFullName) {
    case 'unlink':
      action = (input.length === 0 ? unlink : unlinkPkgs.bind(null, input))
      break
    case 'rebuild':
      action = (input.length === 0 ? rebuild : rebuildPkgs.bind(null, input))
      break
    case 'uninstall':
      action = uninstall.bind(null, input)
      break
    default:
      action = (input.length === 0 ? install : installPkgs.bind(null, input))
      break
  }

  const result = {
    fails: [],
    passes: 0,
  } as RecursiveSummary

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitInstallation(async () => {
        const hooks = opts.ignorePnpmfile ? {} : requireHooks(prefix, opts)
        try {
          const localConfigs = await readLocalConfigs(prefix)
          if (opts.filterByEntryDirectory === prefix) {
            return
          }
          await action({
            ...installOpts,
            ...localConfigs,
            bin: path.join(prefix, 'node_modules', '.bin'),
            hooks,
            prefix,
            rawNpmConfig: {
              ...installOpts.rawNpmConfig,
              ...localConfigs.rawNpmConfig,
            },
            storeController,
          })
          result.passes++
        } catch (err) {
          logger.info(err)

          if (!opts.bail) {
            result.fails.push({
              error: err,
              message: err.message,
              prefix,
            })
            return
          }

          err['prefix'] = prefix // tslint:disable-line:no-string-literal
          throw err
        }
      }),
    ))
  }

  await saveState()

  throwOnFail(result)
}

async function readLocalConfigs (prefix: string) {
  try {
    const ini = await readIniFile(path.join(prefix, '.npmrc'))
    return camelcaseKeys(ini)
  } catch (err) {
    if (err.code !== 'ENOENT') throw err
    return {}
  }
}
