import { getContextForSingleImporter } from '@pnpm/get-context'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { streamParser } from '@pnpm/logger'
import * as dp from 'dependency-path'
import dint = require('dint')
import loadJsonFile = require('load-json-file')
import pFilter = require('p-filter')
import path = require('path')
import extendOptions, {
  StoreStatusOptions,
} from './extendStoreStatusOptions'

export default async function (maybeOpts: StoreStatusOptions) {
  const reporter = maybeOpts?.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }
  const opts = await extendOptions(maybeOpts)
  const {
    registries,
    storeDir,
    skipped,
    virtualStoreDir,
    wantedLockfile,
  } = await getContextForSingleImporter({}, {
    ...opts,
    extraBinPaths: [], // ctx.extraBinPaths is not needed, so this is fine
  })
  if (!wantedLockfile) return []

  const pkgs = Object.keys(wantedLockfile.packages || {})
    .filter((relDepPath) => !skipped.has(relDepPath))
    .map((relDepPath) => {
      return {
        pkgPath: dp.resolve(registries, relDepPath),
        ...nameVerFromPkgSnapshot(relDepPath, wantedLockfile.packages![relDepPath]),
      }
    })

  const modified = await pFilter(pkgs, async ({ pkgPath, name }) => {
    const integrity = await loadJsonFile(path.join(storeDir, pkgPath, 'integrity.json'))
    return (await dint.check(path.join(virtualStoreDir, pkgPath, 'node_modules', name), integrity)) === false
  })

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return modified.map(({ pkgPath }) => pkgPath)
}
