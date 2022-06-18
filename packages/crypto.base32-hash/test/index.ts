/// <reference path="../../../typings/index.d.ts"/>
import { createBase32Hash } from '@pnpm/crypto.base32-hash'

test('createBase32Hash()', () => {
  expect(createBase32Hash('AAA')).toEqual('4h5p7m7gcttmf65hikljmi4gw4')
})
