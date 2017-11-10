import path = require('path')
import {PnpmOptions} from '../types'
import extendOptions from './extendOptions'
import getContext from './getContext'
import logger, {streamParser} from '@pnpm/logger'
import rimraf = require('rimraf-then')
import exists = require('path-exists')
import {Store} from 'package-store'
import R = require('ramda')
import pFilter = require('p-filter')
import pLimit = require('p-limit')
import lock from './lock'
import {save as saveStore} from 'package-store'

export default async function (maybeOpts: PnpmOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  const removedProjects = await getRemovedProject(ctx.storeIndex)

  if (!opts.lock) {
    await run()
  } else {
    await lock(ctx.storePath, run, {stale: opts.lockStaleDuration, locks: opts.locks})
  }

  async function run () {
    for (const pkgId in ctx.storeIndex) {
      ctx.storeIndex[pkgId] = R.difference(ctx.storeIndex[pkgId], removedProjects)

      if (!ctx.storeIndex[pkgId].length) {
        delete ctx.storeIndex[pkgId]
        await rimraf(path.join(ctx.storePath, pkgId))
        logger.info(`- ${pkgId}`)
      }
    }

    await saveStore(ctx.storePath, ctx.storeIndex)
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

const limitExistsCheck = pLimit(10)

async function getRemovedProject (storeIndex: Store) {
  const allProjects = R.uniq(R.unnest<string>(R.values(storeIndex)))

  return await pFilter(allProjects,
    (projectPath: string) => limitExistsCheck(async () => {
      const modulesDir = path.join(projectPath, 'node_modules')
      return !await exists(modulesDir)
    }))
}
