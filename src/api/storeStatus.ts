import path = require('path')
import pFilter = require('p-filter')
import {PnpmOptions} from '../types'
import extendOptions from './extendOptions'
import getContext from './getContext'
import {pkgIsUntouched as untouched} from 'package-store'
import * as dp from 'dependency-path'
import {streamParser} from 'pnpm-logger'

export default async function (maybeOpts: PnpmOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  if (!ctx.wantedShrinkwrap) return []

  const pkgPaths = Object.keys(ctx.wantedShrinkwrap.packages || {})
    .map(id => {
      if (id === '/') return null
      return dp.resolve(ctx.wantedShrinkwrap.registry, id)
    })
    .filter(pkgId => pkgId && !ctx.skipped.has(pkgId))
    .map((pkgPath: string) => path.join(ctx.storePath, pkgPath))

  const modified = await pFilter(pkgPaths, async (pkgPath: string) => !await untouched(path.join(pkgPath, 'package')))

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return modified
}
