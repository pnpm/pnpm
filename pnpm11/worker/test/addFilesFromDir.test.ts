import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterAll, expect, test } from '@jest/globals'
import { PnpmError } from '@pnpm/error'
import type { PackageFilesIndex } from '@pnpm/store.cafs'
import { StoreIndex } from '@pnpm/store.index'

import { addFilesFromDir, finishWorkers } from '../lib/index.js'

afterAll(() => finishWorkers())

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

test('addFilesFromDir() does not cache side effects that contain symlinks', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-worker-test-'))
  const dir = path.join(tmp, 'pkg')
  fs.mkdirSync(dir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'symlink-side-effects-pkg', version: '1.0.0' }))
  const storeDir = path.join(tmp, 'store')
  const filesIndexFile = 'symlink-side-effects-pkg'
  const storeIndex = new StoreIndex(storeDir)

  await addFilesFromDir({
    storeDir,
    dir,
    filesIndexFile,
    storeIndex,
  })

  if (process.platform === 'win32') {
    const targetDir = path.join(dir, 'generated')
    fs.mkdirSync(targetDir)
    fs.writeFileSync(path.join(targetDir, 'index.js'), 'module.exports = true')
    fs.symlinkSync(targetDir, path.join(dir, 'generated-link'), 'junction')
  } else {
    const targetFile = path.join(dir, 'generated.js')
    fs.writeFileSync(targetFile, 'module.exports = true')
    fs.symlinkSync('generated.js', path.join(dir, 'generated-link.js'))
  }

  await addFilesFromDir({
    storeDir,
    dir,
    filesIndexFile,
    sideEffectsCacheKey: 'test-engine',
    storeIndex,
  })

  const filesIndex = storeIndex.get(filesIndexFile) as PackageFilesIndex
  expect(filesIndex.sideEffects).toBeUndefined()
  storeIndex.close()
})

test('addFilesFromDir() ignores the dependency node_modules link when caching side effects', async () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-worker-test-'))
  const dir = path.join(tmp, 'pkg')
  const dependencyDir = path.join(tmp, 'dependency')
  fs.mkdirSync(dir)
  fs.mkdirSync(dependencyDir)
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'regular-side-effects-pkg', version: '1.0.0' }))
  fs.writeFileSync(path.join(dependencyDir, 'index.js'), 'module.exports = true')
  fs.symlinkSync(dependencyDir, path.join(dir, 'node_modules'), process.platform === 'win32' ? 'junction' : 'dir')
  const storeDir = path.join(tmp, 'store')
  const filesIndexFile = 'regular-side-effects-pkg'
  const storeIndex = new StoreIndex(storeDir)

  await addFilesFromDir({
    storeDir,
    dir,
    filesIndexFile,
    storeIndex,
  })
  fs.writeFileSync(path.join(dir, 'generated.js'), 'module.exports = true')
  await addFilesFromDir({
    storeDir,
    dir,
    filesIndexFile,
    sideEffectsCacheKey: 'test-engine',
    storeIndex,
  })

  const filesIndex = storeIndex.get(filesIndexFile) as PackageFilesIndex
  expect(filesIndex.sideEffects?.get('test-engine')?.added?.has('generated.js')).toBe(true)
  storeIndex.close()
})
