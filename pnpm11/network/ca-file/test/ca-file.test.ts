import path from 'node:path'

import { expect, it } from '@jest/globals'
import { readCAFileSync } from '@pnpm/network.ca-file'

it('should read CA file', () => {
  expect(readCAFileSync(path.join(import.meta.dirname, 'fixtures/ca-file1.txt'))).toStrictEqual([
    `-----BEGIN CERTIFICATE-----
XXXX
-----END CERTIFICATE-----`,
    `-----BEGIN CERTIFICATE-----
YYYY
-----END CERTIFICATE-----`,
    `-----BEGIN CERTIFICATE-----
ZZZZ
-----END CERTIFICATE-----`,
  ])
})

it('should not fail when the file does not exist', () => {
  expect(readCAFileSync(path.join(import.meta.dirname, 'not-exists.txt'))).toBeUndefined()
})
