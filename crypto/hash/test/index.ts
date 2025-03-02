/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import { createShortHash, createHashFromFile, getTarballIntegrity } from '@pnpm/crypto.hash'
import { tempDir } from '@pnpm/prepare'
import { pipeline } from 'node:stream/promises'
import tar from 'tar-stream'

test('createShortHash()', () => {
  expect(createShortHash('AAA')).toEqual('cb1ad2119d8fafb69566510ee712661f')
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
    .resolves.toEqual('sha512-nQP7gWOhNQ/5HoM/rJmzOgzZt6Wg6k56CyvO/0sMmiS3UkLSmzY5mW8mMrnbspgqpmOW8q/FHyb0YIr4n2A8VQ==')
})
