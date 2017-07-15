import path = require('path')
import pFilter = require('p-filter')
import {PnpmOptions} from '../types'
import extendOptions from './extendOptions'
import getContext from './getContext'
import {pkgIsUntouched as untouched} from 'package-store'
import * as dp from 'dependency-path'
import streamParser from '../logging/streamParser'

export default async function (maybeOpts: PnpmOptions) {
  const reporter = maybeOpts && maybeOpts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  if (!ctx.shrinkwrap) return []

  const pkgPaths = Object.keys(ctx.shrinkwrap.packages || {})
    .map(id => {
      if (id === '/') return null
      return dp.resolve(ctx.shrinkwrap.registry, id)
    })
    .filter(pkgId => pkgId && !ctx.skipped.has(pkgId))
    .map((pkgPath: string) => path.join(ctx.storePath, pkgPath))

  const modified = await pFilter(pkgPaths, async (pkgPath: string) => !await untouched(path.join(pkgPath, 'package')))

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return modified
}
