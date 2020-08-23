import { getFilePathInCafs, PackageFilesIndex } from '@pnpm/cafs'
import { getContextForSingleImporter } from '@pnpm/get-context'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile-utils'
import { streamParser } from '@pnpm/logger'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import * as dp from 'dependency-path'
import extendOptions, {
  StoreStatusOptions,
} from './extendStoreStatusOptions'
import path = require('path')
import dint = require('dint')
import loadJsonFile = require('load-json-file')
import pFilter = require('p-filter')

export default async function (maybeOpts: StoreStatusOptions) {
  const reporter = maybeOpts?.reporter
  if (reporter && typeof reporter === 'function') {
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

  const pkgs = Object.keys(wantedLockfile.packages ?? {})
    .filter((depPath) => !skipped.has(depPath))
    .map((depPath) => {
      const pkg = wantedLockfile.packages![depPath]
      return {
        depPath,
        integrity: pkg.resolution['integrity'],
        pkgPath: dp.resolve(registries, depPath),
        ...nameVerFromPkgSnapshot(depPath, pkg),
      }
    })

  const cafsDir = path.join(storeDir, 'files')
  const modified = await pFilter(pkgs, async ({ integrity, pkgPath, depPath, name }) => {
    const pkgIndexFilePath = integrity
      ? getFilePathInCafs(cafsDir, integrity, 'index')
      : path.join(storeDir, pkgPath, 'integrity.json')
    const { files } = await loadJsonFile<PackageFilesIndex>(pkgIndexFilePath)
    return (await dint.check(path.join(virtualStoreDir, pkgIdToFilename(depPath, opts.dir), 'node_modules', name), files)) === false
  })

  if (reporter && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  return modified.map(({ pkgPath }) => pkgPath)
}
