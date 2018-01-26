import logger from '@pnpm/logger'
import findPackages from 'find-packages'
import graphSequencer = require('graph-sequencer')
import pLimit = require('p-limit')
import path = require('path')
import createPkgGraph, {PackageNode} from 'pkgs-graph'
import sortPkgs = require('sort-pkgs')
import {
  install,
  InstallOptions,
} from 'supi'
import createStoreController from '../createStoreController'
import requireHooks from '../requireHooks'
import {PnpmOptions} from '../types'

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
  if (cmd && ['install', 'i', 'update', 'up', 'upgrade'].indexOf(cmd) === -1) {
    throw new Error('Unsupported recursive command')
  }
  logger.warn('The recursive command is an experimental feature. Breaking changes may happen in non-major versions.')

  if (cmd === 'update' || cmd === 'up' || cmd === 'upgrade') {
    opts = {...opts, update: true}
  }

  const pkgs = await findPackages(process.cwd(), {
    ignore: [
      '**/node_modules/**',
      '**/bower_components/**',
    ],
  })
  const pkgGraphResult = createPkgGraph(pkgs)
  const graph = new Map(
    Object.keys(pkgGraphResult.graph).map((pkgPath) => [pkgPath, pkgGraphResult.graph[pkgPath].dependencies]) as Array<[string, string[]]>,
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [Object.keys(pkgGraphResult.graph)],
  })
  const chunks = graphSequencerResult.chunks

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
  const installOpts = Object.assign(opts, {
    store: store.path,
    storeController,
  }) as InstallOptions

  const limitInstallation = pLimit(concurrency)

  for (const chunk of chunks) {
    await Promise.all(chunk.map((prefix: string) =>
      limitInstallation(async () => {
        const hooks = opts.ignorePnpmfile ? {} : requireHooks(prefix)
        try {
          return await install({
            ...installOpts,
            bin: path.join(prefix, 'node_modules', '.bin'),
            hooks,
            prefix,
            storeController,
          })
        } catch (err) {
          err['prefix'] = prefix // tslint:disable-line:no-string-literal
          return err
        }
      }),
    ))
  }

  await saveState()
}
