/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import { createShortHash, createHashFromFile } from '@pnpm/crypto.hash'
import { tempDir } from '@pnpm/prepare'

test('createShortHash()', () => {
  expect(createShortHash('AAA')).toEqual('cb1ad2119d8fafb69566510ee712661f')
})

test('createHashFromFile normalizes line endings before calculating the hash', async () => {
  tempDir()
  fs.writeFileSync('win-eol.txt', 'a\r\nb\r\nc')
  fs.writeFileSync('posix-eol.txt', 'a\nb\r\nc')
  expect(await createHashFromFile('win-eol.txt')).toEqual(await createHashFromFile('posix-eol.txt'))
})
