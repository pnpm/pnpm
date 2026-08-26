import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex } from '@pnpm/store.index'

import { addFilesFromDir, finishWorkers } from '../lib/index.js'

afterAll(() => finishWorkers())

test('addFilesFromDir() records whether git preparation was required', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-worker-test-'))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'prepared-pkg', version: '1.0.0' }))
  const storeDir = path.join(tmp, 'store')
  const filesIndexFile = 'prepared-pkg'
  const storeIndex = new StoreIndex(storeDir)

  const result = await addFilesFromDir({
    storeDir,
    dir,
    filesIndexFile,
    requiresPrepare: true,
    storeIndex,
  })

  expect(result.requiresPrepare).toBe(true)
  expect((storeIndex.get(filesIndexFile) as PackageFilesIndex).requiresPrepare).toBe(true)
  storeIndex.close()
})

test('addFilesFromDir() rejects when committing the index writes throws (e.g. a read-only store index)', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-worker-test-'))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'frozen-test-pkg', version: '1.0.0' }))
  const storeDir = path.join(tmp, 'store')

  const storeIndex = {
    setRawMany () {
      throw new PnpmError('FROZEN_STORE_WRITE', 'Cannot write to the package store because frozenStore is enabled')
    },
  } as unknown as StoreIndex

  await expect(addFilesFromDir({
    storeDir,
    dir,
    filesIndexFile: path.join(storeDir, 'frozen-test-pkg.json'),
    storeIndex,
  })).rejects.toMatchObject({ code: 'ERR_PNPM_FROZEN_STORE_WRITE' })
})
