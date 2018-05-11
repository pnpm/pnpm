import logger from '@pnpm/logger'
import camelcaseKeys = require('camelcase-keys')
import findPackages from 'find-packages'
import graphSequencer = require('graph-sequencer')
import loadYamlFile = require('load-yaml-file')
import pLimit = require('p-limit')
import { StoreController } from 'package-store'
import path = require('path')
import createPkgGraph, {PackageNode} from 'pkgs-graph'
import readIniFile = require('read-ini-file')
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

  const cwd = process.cwd()
  const packagesManifest = await requirePackagesManifest(cwd)
  const pkgs = await findPackages(cwd, {
    ignore: [
      '**/node_modules/**',
      '**/bower_components/**',
    ],
    patterns: packagesManifest && packagesManifest.packages || undefined,
  })
  const pkgGraphResult = createPkgGraph(pkgs)
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

  if (cmd === 'link' || cmd === 'ln') {
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
  const action = cmd === 'dislink' ? unlink : install

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
