import path = require('path')
import fs = require('mz/fs')
import pFilter = require('p-filter')
import {PnpmOptions} from '../types'
import extendOptions from './extendOptions'
import getContext from './getContext'
import dirsum from '../fs/dirsum'

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
  return pkgPath.startsWith('/') || pkgPath[1] === ':'
}

async function untouched (pkgDir: string): Promise<Boolean> {
  const realShasum = await dirsum(pkgDir)
  const originalShasum = await fs.readFile(`${pkgDir}_shasum`, 'utf8')
  return realShasum === originalShasum
}
