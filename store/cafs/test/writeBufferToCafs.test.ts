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
})
