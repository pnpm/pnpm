/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import { pipeline } from 'node:stream/promises'

import { expect, test } from '@jest/globals'
import { createHashFromFile, createShortHash, getTarballIntegrity, matchIntegrity } from '@pnpm/crypto.hash'
import { tempDir } from '@pnpm/prepare'
import tar from 'tar-stream'

test('createShortHash()', () => {
  expect(createShortHash('AAA')).toBe('cb1ad2119d8fafb69566510ee712661f')
})

test('createHashFromFile normalizes line endings before calculating the hash', async () => {
  tempDir()
  fs.writeFileSync('win-eol.txt', 'a\r\nb\r\nc')
  fs.writeFileSync('posix-eol.txt', 'a\nb\r\nc')
  expect(await createHashFromFile('win-eol.txt')).toEqual(await createHashFromFile('posix-eol.txt'))
})

test('getTarballIntegrity creates integrity hash for tarball', async () => {
  expect.hasAssertions()
  tempDir()

  const pack = tar.pack()
  pack.entry({ name: 'package.json', mtime: new Date('1970-01-01T00:00:00.000Z') }, JSON.stringify({
    name: 'local-tarball',
    version: '1.0.0',
  }))
  pack.finalize()

  await pipeline(pack, fs.createWriteStream('./local-tarball.tar'))

  await expect(getTarballIntegrity('./local-tarball.tar'))
    .resolves.toBe('sha512-nQP7gWOhNQ/5HoM/rJmzOgzZt6Wg6k56CyvO/0sMmiS3UkLSmzY5mW8mMrnbspgqpmOW8q/FHyb0YIr4n2A8VQ==')

  const multi = await getTarballIntegrity('./local-tarball.tar', {
    algorithms: ['sha512', 'sha1'],
  })
  expect(multi).toContain('sha512-')
  expect(multi).toContain('sha1-')
})

test('matchIntegrity checks SRI algorithms and reports algorithm-specific mismatches', () => {
  const actual = 'sha512-MJ7MSJwS1utMxA9QyQLytNDtd+5RGnx6m808qG1M2G+YndNbxf9JlnDaNCVbRbDP2DDoH2Bdz33FVC6TrpzXbw== sha1-Kq5sNclPz7QV2+lfQIuc6R7oRu0='

  const sha1Match = matchIntegrity(actual, 'sha1-Kq5sNclPz7QV2+lfQIuc6R7oRu0=')
  expect(sha1Match.matches).toBe(true)
  expect(sha1Match.found).toBe('sha1-Kq5sNclPz7QV2+lfQIuc6R7oRu0=')

  const sha512Match = matchIntegrity(actual, 'sha512-MJ7MSJwS1utMxA9QyQLytNDtd+5RGnx6m808qG1M2G+YndNbxf9JlnDaNCVbRbDP2DDoH2Bdz33FVC6TrpzXbw==')
  expect(sha512Match.matches).toBe(true)

  const sha1Mismatch = matchIntegrity(actual, 'sha1-AAAAAAAAAAAAAAAAAAAAAAAAAAA=')
  expect(sha1Mismatch.matches).toBe(false)
  expect(sha1Mismatch.found).toBe('sha1-Kq5sNclPz7QV2+lfQIuc6R7oRu0=')

  const invalid = matchIntegrity(actual, 'invalid-sri')
  expect(invalid.matches).toBe(false)
})
