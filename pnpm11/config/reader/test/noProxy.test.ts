import { expect, test } from '@jest/globals'
import type { PnpmSettings } from '@pnpm/types'

import { getOptionsFromPnpmSettings } from '../src/getOptionsFromRootManifest.js'

test.each([
  [123, 'number'],
  [['localhost'], 'array'],
  [{ host: 'localhost' }, 'object'],
])('rejects invalid noProxy value %j', (value: unknown, receivedType: string) => {
  expect(() => getOptionsFromPnpmSettings(process.cwd(), {
    noProxy: value,
  } as unknown as PnpmSettings)).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_INVALID_SETTING',
    message: `The "noProxy" setting should be a boolean or string, but got ${receivedType}`,
  }))
})

test.each([true, false, '', 'null', 'localhost,127.0.0.1'])('accepts noProxy %j', (value) => {
  const options = getOptionsFromPnpmSettings(process.cwd(), { noProxy: value })
  expect(options.noProxy).toBe(value)
})

test('treats null noProxy as unset', () => {
  const options = getOptionsFromPnpmSettings(process.cwd(), {
    noProxy: null,
  } as unknown as PnpmSettings)
  expect(Object.hasOwn(options, 'noProxy')).toBe(false)
})
