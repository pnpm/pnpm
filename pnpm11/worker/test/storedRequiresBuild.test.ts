import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex } from '@pnpm/store.index'

import { addFilesFromDir, finishWorkers, readPkgFromCafs } from '../lib/index.js'

afterAll(() => finishWorkers())

test('a stored requiresBuild that predates gypfile is rechecked against the package.json in the store', async () => {
  expect(await readLegacyRowRequiresBuild({ name: 'opted-out', version: '1.0.0', gypfile: false })).toBe(false)
  expect(await readLegacyRowRequiresBuild({ name: 'not-opted-out', version: '1.0.0' })).toBe(true)
})

async function readLegacyRowRequiresBuild (manifest: Record<string, unknown>): Promise<boolean> {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-stored-requires-build-'))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest))
  fs.writeFileSync(path.join(dir, 'binding.gyp'), '{}')
  const storeDir = path.join(tmp, 'store')
  const storeIndex = new StoreIndex(storeDir)
  const filesIndexFile = path.join(storeDir, 'pkg.json')
  await addFilesFromDir({ storeDir, dir, filesIndexFile, storeIndex })

  const row = storeIndex.get(filesIndexFile) as PackageFilesIndex
  delete row.manifest?.gypfile
  storeIndex.set(filesIndexFile, { ...row, requiresBuild: true })

  const result = await readPkgFromCafs({ storeDir, verifyStoreIntegrity: false }, filesIndexFile)
  storeIndex.close()
  return result.files.requiresBuild
}
