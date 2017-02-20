import path = require('path')
import pFilter = require('p-filter')
import {PnpmOptions} from '../types'
import extendOptions from './extendOptions'
import getContext from './getContext'
import untouched from '../pkgIsUntouched'
import {GRAPH_ENTRY} from '../fs/graphController'

export default async function (maybeOpts: PnpmOptions) {
  const opts = extendOptions(maybeOpts)
  const ctx = await getContext(opts)
  if (!ctx.graph) return []

  const pkgPaths = Object.keys(ctx.graph)
    .filter(pkgPath => !isProjectPath(pkgPath))
    .map(pkgPath => path.join(ctx.storePath, pkgPath))

  return await pFilter(pkgPaths, async (pkgPath: string) => !await untouched(pkgPath))
}

function isProjectPath (pkgPath: string) {
  return pkgPath === GRAPH_ENTRY ||
    // next are for backward compatibility
    // previous versions of .graph.yaml had the package path as the entry point
    pkgPath.startsWith('/') || pkgPath[1] === ':'
}
