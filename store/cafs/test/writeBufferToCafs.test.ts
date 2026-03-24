import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { temporaryDirectory } from 'tempy'

import { writeBufferToCafs } from '../src/writeBufferToCafs.js'

describe('writeBufferToCafs', () => {
  it('should write directly to the final CAS path', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', buffer, 'hex')
    writeBufferToCafs(new Map(), storeDir, buffer, fileDest, 420, { digest, algorithm: 'sha512' })
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe('abc')
  })

  it('should populate the locker cache when a file already exists with correct integrity', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const digest = crypto.hash('sha512', buffer, 'hex')
    const integrity = { digest, algorithm: 'sha512' }
    const locker = new Map<string, number>()

    // First write creates the file
    writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    // Clear the locker to simulate a fresh lookup
    locker.clear()

    // Second call should find the file on disk and cache it
    const result = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    const fullFileDest = path.join(storeDir, fileDest)
    expect(locker.has(fullFileDest)).toBe(true)

    // Third call should return from locker cache without hitting disk
    const cached = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    expect(cached.checkedAt).toBe(result.checkedAt)
  })

  it('should handle EEXIST race by verifying integrity of the existing file', () => {
    const storeDir = temporaryDirectory()
    const fileDest = 'abc'
    const buffer = Buffer.from('abc')
    const fullFileDest = path.join(storeDir, fileDest)
    const digest = crypto.hash('sha512', buffer, 'hex')
    const integrity = { digest, algorithm: 'sha512' }
    const locker = new Map<string, number>()

    // Simulate another process creating the file between our stat and write
    // by pre-creating it with correct content
    fs.mkdirSync(path.dirname(fullFileDest), { recursive: true })
    fs.writeFileSync(fullFileDest, buffer)

    // Our write should detect the file via stat (not EEXIST), verify integrity, and cache
    const result = writeBufferToCafs(locker, storeDir, buffer, fileDest, 420, integrity)
    expect(result.filePath).toBe(fullFileDest)
    expect(locker.has(fullFileDest)).toBe(true)
    expect(fs.readFileSync(fullFileDest, 'utf8')).toBe('abc')
  })
})
