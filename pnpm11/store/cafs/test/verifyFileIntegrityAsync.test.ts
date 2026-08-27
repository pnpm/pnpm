import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import { verifyFileIntegrityAsync } from '../src/index.js'

const sha512 = (data: string): string => crypto.createHash('sha512').update(data).digest('hex')

describe('verifyFileIntegrityAsync', () => {
  it('accepts content that hashes to the digest and rejects content that does not', async () => {
    const dir = temporaryDirectory()
    const file = path.join(dir, 'addon.node')
    fs.writeFileSync(file, 'addon')
    const digest = sha512('addon')
    await expect(verifyFileIntegrityAsync(file, { algorithm: 'sha512', digest })).resolves.toBe(true)

    fs.writeFileSync(file, 'tampered')
    await expect(verifyFileIntegrityAsync(file, { algorithm: 'sha512', digest })).resolves.toBe(false)
  })

  it('hashes a file larger than one read chunk', async () => {
    const dir = temporaryDirectory()
    const file = path.join(dir, 'big.node')
    const content = 'x'.repeat(64 * 1024 * 3 + 17)
    fs.writeFileSync(file, content)
    await expect(
      verifyFileIntegrityAsync(file, { algorithm: 'sha512', digest: sha512(content) })
    ).resolves.toBe(true)
  })

  it.each([
    ['a path that does not exist', (dir: string) => path.join(dir, 'absent')],
    ['a path that is not a regular file', (dir: string) => dir],
  ])('reports %s as unverified rather than throwing', async (_name, target) => {
    const dir = temporaryDirectory()
    await expect(
      verifyFileIntegrityAsync(target(dir), { algorithm: 'sha512', digest: sha512('addon') })
    ).resolves.toBe(false)
  })

  it('reports an unusable algorithm as unverified', async () => {
    const dir = temporaryDirectory()
    const file = path.join(dir, 'addon.node')
    fs.writeFileSync(file, 'addon')
    await expect(
      verifyFileIntegrityAsync(file, { algorithm: 'not-an-algorithm', digest: sha512('addon') })
    ).resolves.toBe(false)
  })
})
