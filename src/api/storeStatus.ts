import path = require('path')
import pFilter = require('p-filter')
import {PnpmOptions} from '../types'
import extendOptions from './extendOptions'
import getContext from './getContext'
import untouched from '../pkgIsUntouched'
import {shortIdToFullId} from '../fs/shrinkwrap'

export default async function (maybeOpts: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  if (!ctx.shrinkwrap) return []

  const pkgPaths = Object.keys(ctx.shrinkwrap.packages || {})
    .map(id => shortIdToFullId(id, ctx.shrinkwrap.registry))
    .map(pkgPath => path.join(ctx.storePath, pkgPath))

  return await pFilter(pkgPaths, async (pkgPath: string) => !await untouched(pkgPath))
}
