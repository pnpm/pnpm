import logger from '@pnpm/logger'
import camelcaseKeys = require('camelcase-keys')
import graphSequencer = require('graph-sequencer')
import minimatch = require('minimatch')
import pLimit = require('p-limit')
import { StoreController } from 'package-store'
import path = require('path')
import createPkgGraph, {PackageNode} from 'pkgs-graph'
import R = require('ramda')
import readIniFile = require('read-ini-file')
import {
  install,
  InstallOptions,
  installPkgs,
  link,
  rebuild,
  rebuildPkgs,
  uninstall,
  unlink,
  unlinkPkgs,
} from 'supi'
import createStoreController from '../../createStoreController'
import findWorkspacePackages from '../../findWorkspacePackages'
import getCommandFullName from '../../getCommandFullName'
import requireHooks from '../../requireHooks'
import {PnpmOptions} from '../../types'
import help from '../help'
import exec from './exec'
import list from './list'
import outdated from './outdated'
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
  let concurrency = 4
  if (!isNaN(parseInt(input[0], 10))) {
    concurrency = parseInt(input.shift() as string, 10)
  }

  if (concurrency < 1) {
    throw new Error('Concurrency should be at least 1')
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
  logger.warn('The recursive command is an experimental feature. Breaking changes may happen in non-major versions.')

  const cwd = process.cwd()
  let pkgs = await findWorkspacePackages(cwd)

  const pkgGraphResult = createPkgGraph(pkgs)
  if (opts.scope) {
    pkgGraphResult.graph = filterGraph(pkgGraphResult.graph, opts.scope)
    pkgs = pkgs.filter((pkg: {path: string}) => pkgGraphResult.graph[pkg.path])
  }

  switch (cmdFullName) {
    case 'list':
      await list(pkgs, input, cmd, opts as any) // tslint:disable-line:no-any
      return
    case 'outdated':
      await outdated(pkgs, input, cmd, opts as any) // tslint:disable-line:no-any
      return
    case 'test':
      return run(pkgs, ['test', ...input], cmd, {...opts, concurrency} as any) // tslint:disable-line:no-any
    case 'run':
      return run(pkgs, input, cmd, {...opts, concurrency} as any) // tslint:disable-line:no-any
    case 'update':
      opts = {...opts, update: true, allowNew: false, concurrency} as any // tslint:disable-line:no-any
      break
    case 'exec':
      return exec(pkgs, input, cmd, {...opts, concurrency} as any) // tslint:disable-line:no-any
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

  if (cmdFullName === 'link') {
    await linkPackages(pkgGraphResult.graph, {
      registry: opts.registry,
      store: store.path,
      storeController,
    })
  }
  const graph = new Map(
    Object.keys(pkgGraphResult.graph).map((pkgPath) => [pkgPath, pkgGraphResult.graph[pkgPath].dependencies]) as Array<[string, string[]]>,
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [Object.keys(pkgGraphResult.graph)],
  })
  const chunks = graphSequencerResult.chunks

  const installOpts = Object.assign(opts, {
    ownLifecycleHooksStdio: 'pipe',
    store: store.path,
    storeController,
  }) as InstallOptions

  const limitInstallation = pLimit(concurrency)
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

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitInstallation(async () => {
        const hooks = opts.ignorePnpmfile ? {} : requireHooks(prefix, opts)
        try {
          const localConfigs = await readLocalConfigs(prefix)
          return await action({
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
        } catch (err) {
          logger.info(err)
          err['prefix'] = prefix // tslint:disable-line:no-string-literal
          throw err
        }
      }),
    ))
  }

  await saveState()
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

function linkPackages (
  graph: {[pkgPath: string]: {dependencies: string[]}},
  opts: {
    registry?: string,
    store: string,
    storeController: StoreController,
  },
) {
  const limitLinking = pLimit(12)
  const linkOpts = {...opts, skipInstall: true}
  return Promise.all(
    Object.keys(graph)
      .filter((pkgPath) => graph[pkgPath].dependencies && graph[pkgPath].dependencies.length)
      .map((pkgPath) =>
        limitLinking(() =>
          link(graph[pkgPath].dependencies, path.join(pkgPath, 'node_modules'), {...linkOpts, prefix: pkgPath}),
        ),
      ),
  )
}

interface PackageGraph {
  [id: string]: PackageNode,
}

function filterGraph (
  graph: PackageGraph,
  scope: string,
): PackageGraph {
  const root = R.keys(graph).filter((id) => graph[id].manifest.name && minimatch(graph[id].manifest.name, scope))
  if (!root.length) return {}

  const subgraphNodeIds = new Set()
  pickSubgraph(graph, root, subgraphNodeIds)

  return R.pick(Array.from(subgraphNodeIds), graph)
}

function pickSubgraph (
  graph: PackageGraph,
  nextNodeIds: string[],
  walked: Set<string>,
) {
  for (const nextNodeId of nextNodeIds) {
    if (!walked.has(nextNodeId)) {
      walked.add(nextNodeId)
      pickSubgraph(graph, graph[nextNodeId].dependencies, walked)
    }
  }
}
