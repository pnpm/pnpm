import { getFilePathInCafs } from '@pnpm/cafs'
import { getContextForSingleImporter } from '@pnpm/get-context'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { streamParser } from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
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
      const pkg = wantedLockfile.packages![relDepPath]
      return {
        integrity: pkg.resolution['integrity'],
        pkgPath: dp.resolve(registries, relDepPath),
        ...nameVerFromPkgSnapshot(relDepPath, pkg),
      }
    })

  const cafsDir = path.join(storeDir, 'files')
  const modified = await pFilter(pkgs, async ({ integrity, pkgPath, name }) => {
    const pkgIndexFilePath = integrity
      ? getFilePathInCafs(cafsDir, integrity, 'index')
      : path.join(storeDir, pkgPath, 'integrity.json')
    const pkgIndex = await loadJsonFile(pkgIndexFilePath)
    return (await dint.check(path.join(virtualStoreDir, pkgIdToFilename(pkgPath, opts.dir), 'node_modules', name), pkgIndex)) === false
  })

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }

  return modified.map(({ pkgPath }) => pkgPath)
}
