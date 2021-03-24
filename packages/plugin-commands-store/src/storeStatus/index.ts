import path from 'path'
import { getFilePathInCafs, PackageFilesIndex } from '@pnpm/cafs'
import { getContextForSingleImporter } from '@pnpm/get-context'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
} from '@pnpm/lockfile-utils'
import { streamParser } from '@pnpm/logger'
import * as dp from 'dependency-path'
import dint from 'dint'
import loadJsonFile from 'load-json-file'
import pFilter from 'p-filter'
import extendOptions, {
  StoreStatusOptions,
} from './extendStoreStatusOptions'

export default async function (maybeOpts: StoreStatusOptions) {
  const reporter = maybeOpts?.reporter
  if ((reporter != null) && typeof reporter === 'function') {
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
      const pkgSnapshot = wantedLockfile.packages![depPath]
      const id = packageIdFromSnapshot(depPath, pkgSnapshot, registries)
      return {
        depPath,
        id,
        integrity: pkgSnapshot.resolution['integrity'],
        pkgPath: dp.resolve(registries, depPath),
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
      }
    })

  const cafsDir = path.join(storeDir, 'files')
  const modified = await pFilter(pkgs, async ({ id, integrity, depPath, name }) => {
    const pkgIndexFilePath = integrity
      ? getFilePathInCafs(cafsDir, integrity, 'index')
      : path.join(storeDir, dp.depPathToFilename(id, opts.dir), 'integrity.json')
    const { files } = await loadJsonFile<PackageFilesIndex>(pkgIndexFilePath)
    return (await dint.check(path.join(virtualStoreDir, dp.depPathToFilename(depPath, opts.dir), 'node_modules', name), files)) === false
  }, { concurrency: 8 })

  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  return modified.map(({ pkgPath }) => pkgPath)
}
