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
      maxTarballUnpackedSize: 4294967296,
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
      maxTarballUnpackedSize: 4294967296,
    })
    expect(result).toMatchObject({ status: 'success', value: { integrity: source.integrity } })
    expect('indexWrites' in result && result.indexWrites).toHaveLength(pinned ? 1 : 2)
  } finally {
    fs.rmSync(source.temporaryDirectory, { recursive: true, force: true })
  }
})

function createTarEntry (name: string, content: string): Buffer {
  const header = Buffer.alloc(512)
  header.write(name, 0)
  header.write('0000644\0', 100)
  header.write(content.length.toString(8).padStart(11, '0') + '\0', 124)
  header.fill(' ', 148, 156)
  header[156] = '0'.charCodeAt(0)
  header.write(`${header.reduce((sum, byte) => sum + byte, 0).toString(8).padStart(6, '0')}\0 `, 148)
  const body = Buffer.alloc(Math.ceil(content.length / 512) * 512)
  body.write(content)
  return Buffer.concat([header, body])
}

test.each([false, true])('an archive that unpacks past maxTarballUnpackedSize is rejected with a hint (file-backed: %s)', async (fileBacked) => {
  const temporaryDirectory = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-tarball-limit-'))
  try {
    const buffer = gzipSync(Buffer.concat([
      createTarEntry('package/package.json', '{"name":"limited","version":"1.0.0"}'),
      createTarEntry('package/index.js', 'x'.repeat(100)),
      Buffer.alloc(1024),
    ]))
    const tarballFile = path.join(temporaryDirectory, 'archive.tgz')
    fs.writeFileSync(tarballFile, buffer)
    await expect(addTarballToStore({
      type: 'extract',
      ...(fileBacked ? { tarballFile } : { buffer }),
      storeDir: path.join(temporaryDirectory, 'store'),
      filesIndexFile: 'test',
      maxTarballUnpackedSize: 100,
    })).rejects.toMatchObject({
      code: 'ERR_PNPM_TARBALL_TOO_LARGE',
      hint: 'If the package is legitimately this large, raise the maxTarballUnpackedSize setting.',
    })
  } finally {
    fs.rmSync(temporaryDirectory, { recursive: true, force: true })
  }
})
