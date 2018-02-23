import logger from '@pnpm/logger'
import findPackages from 'find-packages'
import graphSequencer = require('graph-sequencer')
import loadYamlFile = require('load-yaml-file')
import pLimit = require('p-limit')
import { StoreController } from 'package-store'
import path = require('path')
import createPkgGraph, {PackageNode} from 'pkgs-graph'
import sortPkgs = require('sort-pkgs')
import {
  install,
  InstallOptions,
  link,
  unlink,
} from 'supi'
import createStoreController from '../createStoreController'
import requireHooks from '../requireHooks'
import {PnpmOptions} from '../types'

const supportedRecursiveCommands = new Set([
  'install',
  'i',
  'update',
  'up',
  'upgrade',
  'link',
  'ln',
  'dislink',
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
  if (cmd && !supportedRecursiveCommands.has(cmd)) {
    throw new Error('Unsupported recursive command')
  }
  logger.warn('The recursive command is an experimental feature. Breaking changes may happen in non-major versions.')

  if (cmd === 'update' || cmd === 'up' || cmd === 'upgrade') {
    opts = {...opts, update: true}
  }

  const packagesManifest = await requirePackagesManifest(opts.prefix)
  const pkgs = await findPackages(opts.prefix, {
    ignore: [
      '**/node_modules/**',
      '**/bower_components/**',
    ],
    patterns: packagesManifest && packagesManifest.packages || undefined,
  })
  const pkgGraphResult = createPkgGraph(pkgs)
  const store = await createStoreController(opts)
  if (cmd === 'link' || cmd === 'ln') {
    await linkPackages(pkgGraphResult.graph, store.ctrl, store.path)
  }
  const graph = new Map(
    Object.keys(pkgGraphResult.graph).map((pkgPath) => [pkgPath, pkgGraphResult.graph[pkgPath].dependencies]) as Array<[string, string[]]>,
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [Object.keys(pkgGraphResult.graph)],
  })
  const chunks = graphSequencerResult.chunks

  // It is enough to save the store.json file once,
  // once all installations are done.
  // That's why saveState that is passed to the install engine
  // does nothing.
  const saveState = store.ctrl.saveState
  const storeController = {
    ...store.ctrl,
    saveState: async () => undefined,
  }
  const installOpts = Object.assign(opts, {
    store: store.path,
    storeController,
  }) as InstallOptions

  const limitInstallation = pLimit(concurrency)
  const action = cmd === 'dislink' ? unlink : install

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitInstallation(async () => {
        const hooks = opts.ignorePnpmfile ? {} : requireHooks(prefix)
        try {
          return await action({
            ...installOpts,
            bin: path.join(prefix, 'node_modules', '.bin'),
            hooks,
            prefix,
            storeController,
          })
        } catch (err) {
          logger.info(err)
          err['prefix'] = prefix // tslint:disable-line:no-string-literal
          return err
        }
      }),
    ))
  }

  await saveState()
}

function linkPackages (
  graph: {[pkgPath: string]: {dependencies: string[]}},
  storeController: StoreController,
  store: string,
) {
  const limitLinking = pLimit(12)
  const linkOpts = {skipInstall: true, store, storeController}
  return Promise.all(
    Object.keys(graph)
      .map((pkgPath) => Promise.all(
        (graph[pkgPath].dependencies || [])
          .map((depPath) => limitLinking(() => link(depPath, pkgPath, linkOpts))))),
  )
}

async function requirePackagesManifest (dir: string): Promise<{packages: string[]} | null> {
  try {
    return await loadYamlFile(path.join(dir, 'pnpm-workspace.yaml')) as {packages: string[]}
  } catch (err) {
    if (err['code'] === 'ENOENT') { // tslint:disable-line
      return null
    }
    throw err
  }
}
