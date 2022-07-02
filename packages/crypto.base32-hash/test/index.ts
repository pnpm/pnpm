/// <reference path="../../../typings/index.d.ts"/>
import fs from 'fs'
import { createBase32Hash, createBase32HashFromFile } from '@pnpm/crypto.base32-hash'
import { tempDir } from '@pnpm/prepare'

test('createBase32Hash()', () => {
  expect(createBase32Hash('AAA')).toEqual('4h5p7m7gcttmf65hikljmi4gw4')
})

test('createBase32HashFromFile normalizes line endings before calculating the hash', async () => {
  tempDir()
  fs.writeFileSync('win-eol.txt', 'a\r\nb\r\nc')
  fs.writeFileSync('posix-eol.txt', 'a\nb\r\nc')
  expect(await createBase32HashFromFile('win-eol.txt')).toEqual(await createBase32HashFromFile('posix-eol.txt'))
})
