import { execFileSync } from 'node:child_process'
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

  // Windows has no `O_NOFOLLOW`, and creating a symlink there needs a
  // privilege the test runner is not guaranteed.
  const itPosix = process.platform === 'win32' ? it.skip : it
  itPosix('refuses a symlink even when it names content with the right digest', async () => {
    const dir = temporaryDirectory()
    const outside = path.join(dir, 'outside')
    const link = path.join(dir, 'linked')
    fs.writeFileSync(outside, 'addon')
    fs.symlinkSync(outside, link)
    await expect(
      verifyFileIntegrityAsync(link, { algorithm: 'sha512', digest: sha512('addon') })
    ).resolves.toBe(false)
  })

  itPosix('does not hang on a FIFO planted at the path', async () => {
    const dir = temporaryDirectory()
    const fifo = path.join(dir, 'fifo')
    execFileSync('mkfifo', [fifo])
    await expect(
      verifyFileIntegrityAsync(fifo, { algorithm: 'sha512', digest: sha512('addon') })
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
