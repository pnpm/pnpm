import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, describe, expect, it } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import {
  checkPkgFilesIntegrity,
  createCafs,
  getFilePathByModeInCafs,
  takeVerifiedFileIntegrity,
} from '../src/index.js'

describe('the verified-file-integrity tally', () => {
  beforeEach(() => {
    takeVerifiedFileIntegrity()
  })

  it('records a file whose content was re-hashed', () => {
    const { storeDir, digest, size } = storeWithOneFile()

    expect(checkPkgFilesIntegrity(storeDir, indexEntry(digest, size)).passed).toBe(true)

    const tallied = takeVerifiedFileIntegrity()
    expect(tallied.files).toBe(1)
    expect(tallied.ms).toBeGreaterThan(0)
  })

  // Same length, different content: a size mismatch is rejected before
  // anything is hashed, so only equal-length tampering reaches the hash.
  it('records a digest mismatch, which hashed the file to find out', () => {
    const { storeDir, filePath, digest, size } = storeWithOneFile()
    fs.writeFileSync(filePath, 'bar\n')

    expect(checkPkgFilesIntegrity(storeDir, indexEntry(digest, size)).passed).toBe(false)

    expect(takeVerifiedFileIntegrity().files).toBe(1)
  })

  it('records nothing for a file the index says is untouched', () => {
    const { storeDir, digest, size } = storeWithOneFile()
    const trusted = indexEntry(digest, size)
    trusted.files.get('foo.txt')!.checkedAt = Date.now() + 60_000

    expect(checkPkgFilesIntegrity(storeDir, trusted).passed).toBe(true)

    expect(takeVerifiedFileIntegrity()).toEqual({ files: 0, ms: 0 })
  })

  it('records nothing for a missing file', () => {
    const { storeDir, filePath, digest, size } = storeWithOneFile()
    fs.rmSync(filePath)

    expect(checkPkgFilesIntegrity(storeDir, indexEntry(digest, size)).passed).toBe(false)

    expect(takeVerifiedFileIntegrity()).toEqual({ files: 0, ms: 0 })
  })

  it('records nothing for an algorithm it cannot hash with', () => {
    const { storeDir, digest, size } = storeWithOneFile()

    expect(checkPkgFilesIntegrity(storeDir, indexEntry(digest, size, 'not-an-algo')).passed).toBe(false)

    expect(takeVerifiedFileIntegrity()).toEqual({ files: 0, ms: 0 })
  })
})

/** A store holding one file. */
function storeWithOneFile (): { storeDir: string, filePath: string, digest: string, size: number } {
  const storeDir = temporaryDirectory()
  const srcDir = path.join(import.meta.dirname, 'fixtures/one-file')
  const { filesIndex } = createCafs(storeDir).addFilesFromDir(srcDir)
  const { digest, size } = filesIndex.get('foo.txt')!
  return { storeDir, filePath: getFilePathByModeInCafs(storeDir, digest, 420), digest, size }
}

/**
 * The index row describing that file, with `checkedAt` a minute in the
 * past so the file counts as modified — which is what makes
 * verification re-hash its content instead of trusting the row.
 */
function indexEntry (digest: string, size: number, algo = 'sha512') {
  return {
    algo,
    files: new Map([['foo.txt', { digest, mode: 420, size, checkedAt: Date.now() - 60_000 }]]),
  }
}
