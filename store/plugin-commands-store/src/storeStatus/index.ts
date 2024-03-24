import path from 'node:path'

import dint from 'dint'
import pFilter from 'p-filter'
import loadJsonFile from 'load-json-file'

import { streamParser } from '@pnpm/logger'
import * as dp from '@pnpm/dependency-path'
import { getFilePathInCafs } from '@pnpm/store.cafs'
import { getContextForSingleImporter } from '@pnpm/get-context'
import type { TarballResolution, PackageFilesIndex } from '@pnpm/types'
import { nameVerFromPkgSnapshot, packageIdFromSnapshot } from '@pnpm/lockfile-utils'

import { extendStoreStatusOptions, type StoreStatusOptions } from './extendStoreStatusOptions.js'

export async function storeStatus(maybeOpts: StoreStatusOptions): Promise<string[]> {
  const reporter = maybeOpts?.reporter
  if (reporter != null && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }
  const opts = await extendStoreStatusOptions(maybeOpts)
  const { registries, storeDir, skipped, virtualStoreDir, wantedLockfile } =
    await getContextForSingleImporter(
      {},
      {
        ...opts,
        extraBinPaths: [], // ctx.extraBinPaths is not needed, so this is fine
      }
    )
  if (!wantedLockfile) return []

  const pkgs = Object.entries(wantedLockfile.packages ?? {})
    .filter(([depPath]) => !skipped.has(depPath))
    .map(([depPath, pkgSnapshot]) => {
      const id = packageIdFromSnapshot(depPath, pkgSnapshot, registries)
      return {
        depPath,
        id,
        integrity: (pkgSnapshot.resolution as TarballResolution).integrity,
        pkgPath: dp.resolve(registries, depPath),
        ...nameVerFromPkgSnapshot(depPath, pkgSnapshot),
      }
    })

  const cafsDir = path.join(storeDir, 'files')
  const modified = await pFilter(
    pkgs,
    async ({ id, integrity, depPath, name }) => {
      const pkgIndexFilePath = integrity
        ? getFilePathInCafs(cafsDir, integrity, 'index')
        : path.join(storeDir, dp.depPathToFilename(id), 'integrity.json')
      const { files } = await loadJsonFile<PackageFilesIndex>(pkgIndexFilePath)
      return (
        (await dint.check(
          path.join(
            virtualStoreDir,
            dp.depPathToFilename(depPath),
            'node_modules',
            name
          ),
          files
        )) === false
      )
    },
    { concurrency: 8 }
  )

  if (reporter != null && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }

  return modified.map(({ pkgPath }) => pkgPath)
}
