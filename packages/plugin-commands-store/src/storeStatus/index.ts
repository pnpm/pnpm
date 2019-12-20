import checkPackage from '@pnpm/check-package'
import { getContextForSingleImporter } from '@pnpm/get-context'
import { streamParser } from '@pnpm/logger'
import * as dp from 'dependency-path'
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
    wantedLockfile,
  } = await getContextForSingleImporter({}, {
    ...opts,
    extraBinPaths: [], // ctx.extraBinPaths is not needed, so this is fine
  })
  if (!wantedLockfile) return []

  const pkgPaths = (Object.keys(wantedLockfile.packages || {})
    .map((id) => {
      if (id === '/') return null
      return dp.resolve(registries, id)
    })
    .filter((pkgId) => pkgId && !skipped.has(pkgId)) as string[])
    .map((pkgPath: string) => path.join(storeDir, pkgPath))

  const modified = await pFilter(pkgPaths, async (pkgPath: string) => !await checkPackage(path.join(pkgPath, 'package')))

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return modified
}
