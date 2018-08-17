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
  let pkgs = await findWorkspacePackages(cwd)

  const pkgGraphResult = createPkgGraph(pkgs)
  if (opts.scope) {
    pkgGraphResult.graph = filterGraphByScope(pkgGraphResult.graph, opts.scope)
    pkgs = pkgs.filter((pkg: {path: string}) => pkgGraphResult.graph[pkg.path])
  } else if (opts.filter) {
    pkgGraphResult.graph = filterGraph(pkgGraphResult.graph, opts.filter)
    pkgs = pkgs.filter((pkg: {path: string}) => pkgGraphResult.graph[pkg.path])
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

function linkPackages (
  graph: {[pkgPath: string]: {dependencies: string[]}},
  opts: {
    registry?: string,
    store: string,
    storeController: StoreController,
  },
) {
  const limitLinking = pLimit(12)
  return Promise.all(
    Object.keys(graph)
      .filter((pkgPath) => graph[pkgPath].dependencies && graph[pkgPath].dependencies.length)
      .map((pkgPath) =>
        limitLinking(() =>
          link(graph[pkgPath].dependencies, path.join(pkgPath, 'node_modules'), {...opts, prefix: pkgPath}),
        ),
      ),
  )
}

interface PackageGraph {
  [id: string]: PackageNode,
}

interface Graph {
  [nodeId: string]: string[],
}

function filterGraph (
  pkgGraph: PackageGraph,
  filters: string[],
): PackageGraph {
  const cherryPickedPackages = [] as string[]
  const walkedDependencies = new Set<string>()
  const walkedDependents = new Set<string>()
  const graph = pkgGraphToGraph(pkgGraph)
  let reversedGraph: Graph | undefined
  for (const filter of filters) {
    if (filter.endsWith('...')) {
      const rootPackagesFilter = filter.substring(0, filter.length - 3)
      const rootPackages = matchPackages(pkgGraph, rootPackagesFilter)
      pickSubgraph(graph, rootPackages, walkedDependencies)
    } else if (filter.startsWith('...')) {
      const leafPackagesFilter = filter.substring(3)
      const leafPackages = matchPackages(pkgGraph, leafPackagesFilter)
      if (!reversedGraph) {
        reversedGraph = reverseGraph(graph)
      }
      pickSubgraph(reversedGraph, leafPackages, walkedDependents)
    } else {
      Array.prototype.push.apply(cherryPickedPackages, matchPackages(pkgGraph, filter))
    }
  }
  const walked = new Set([...walkedDependencies, ...walkedDependents])
  cherryPickedPackages.forEach((cherryPickedPackage) => walked.add(cherryPickedPackage))
  return R.pick(Array.from(walked), pkgGraph)
}

function pkgGraphToGraph (pkgGraph: PackageGraph): Graph {
  const graph: Graph = {}
  Object.keys(pkgGraph).forEach((nodeId) => {
    graph[nodeId] = pkgGraph[nodeId].dependencies
  })
  return graph
}

function reverseGraph (graph: Graph): Graph {
  const reversedGraph: Graph = {}
  Object.keys(graph).forEach((dependentNodeId) => {
    graph[dependentNodeId].forEach((dependencyNodeId) => {
      if (!reversedGraph[dependencyNodeId]) {
        reversedGraph[dependencyNodeId] = [dependentNodeId]
      } else {
        reversedGraph[dependencyNodeId].push(dependentNodeId)
      }
    })
  })
  return reversedGraph
}

function matchPackages (
  graph: PackageGraph,
  pattern: string,
) {
  return R.keys(graph).filter((id) => graph[id].manifest.name && minimatch(graph[id].manifest.name, pattern))
}

function filterGraphByScope (
  graph: PackageGraph,
  scope: string,
): PackageGraph {
  const root = matchPackages(graph, scope)
  if (!root.length) return {}

  const subgraphNodeIds = new Set()
  pickSubPkgGraph(graph, root, subgraphNodeIds)

  return R.pick(Array.from(subgraphNodeIds), graph)
}

function pickSubPkgGraph (
  graph: PackageGraph,
  nextNodeIds: string[],
  walked: Set<string>,
) {
  for (const nextNodeId of nextNodeIds) {
    if (!walked.has(nextNodeId)) {
      walked.add(nextNodeId)
      pickSubPkgGraph(graph, graph[nextNodeId].dependencies, walked)
    }
  }
}

function pickSubgraph (
  graph: Graph,
  nextNodeIds: string[],
  walked: Set<string>,
) {
  for (const nextNodeId of nextNodeIds) {
    if (!walked.has(nextNodeId)) {
      walked.add(nextNodeId)
      if (graph[nextNodeId]) pickSubgraph(graph, graph[nextNodeId], walked)
    }
  }
}
