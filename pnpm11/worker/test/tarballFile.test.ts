import crypto from 'node:crypto'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { gzipSync } from 'node:zlib'

import { expect, test } from '@jest/globals'

import { addTarballToStore } from '../src/addToStore.js'

function createSource (): { temporaryDirectory: string, tarballFile: string, storeDir: string, integrity: string } {
  const temporaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-tarball-file-'))
  const tarballFile = path.join(temporaryDirectory, 'archive.tgz')
  const data = gzipSync(Buffer.alloc(1024))
  fs.writeFileSync(tarballFile, data)
  return {
    temporaryDirectory,
    tarballFile,
    storeDir: path.join(temporaryDirectory, 'store'),
    integrity: `sha512-${crypto.hash('sha512', data, 'base64')}`,
  }
}

test('file-backed tarballs are verified before extraction', async () => {
  const source = createSource()
  try {
    const result = await addTarballToStore({
      type: 'extract',
      tarballFile: source.tarballFile,
      storeDir: source.storeDir,
      filesIndexFile: 'test',
      integrity: `sha512-${crypto.hash('sha512', 'wrong', 'base64')}`,
    })
    expect(result).toMatchObject({ status: 'error', error: { type: 'integrity_validation_failed', found: source.integrity } })
    expect(fs.existsSync(source.storeDir)).toBe(false)
  } finally {
    fs.rmSync(source.temporaryDirectory, { recursive: true, force: true })
  }
})

test.each([false, true])('file-backed extraction retains the tarball integrity (pinned: %s)', async (pinned) => {
  const source = createSource()
  try {
    const result = await addTarballToStore({
      type: 'extract',
      tarballFile: source.tarballFile,
      storeDir: source.storeDir,
      filesIndexFile: 'test',
      pkgId: 'test@1.0.0',
      integrity: pinned ? source.integrity : undefined,
    })
    expect(result).toMatchObject({ status: 'success', value: { integrity: source.integrity } })
    expect('indexWrites' in result && result.indexWrites).toHaveLength(pinned ? 1 : 2)
  } finally {
    fs.rmSync(source.temporaryDirectory, { recursive: true, force: true })
  }
})
