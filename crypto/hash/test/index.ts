/// <reference path="../../../__typings__/index.d.ts"/>
import { createShortHash } from '@pnpm/crypto.hash'

test('createShortHash()', () => {
  expect(createShortHash('AAA')).toEqual('cb1ad2119d8fafb69566510ee712661f')
})
