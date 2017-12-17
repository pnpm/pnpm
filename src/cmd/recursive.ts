import logger from '@pnpm/logger'
import findPackages from 'find-packages'
import pLimit = require('p-limit')
import createPkgGraph, {PackageNode} from 'pkgs-graph'
import sortPkgs = require('sort-pkgs')
import {
  install,
  PnpmOptions,
} from 'supi'
import createStore from '../createStore'

export default async (input: string[], opts: PnpmOptions) => {
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

  const pkgs = await findPackages(process.cwd())
  const graph = createPkgGraph(pkgs)
  const sortedPkgNodes = sortPkgs(graph)

  const store = await createStore(opts)

  const limitInstallation = pLimit(concurrency)
  await sortedPkgNodes.map((pkg: PackageNode) =>
    limitInstallation(() => install({...opts, storeController: store.ctrl, prefix: pkg.path})),
  )
}
