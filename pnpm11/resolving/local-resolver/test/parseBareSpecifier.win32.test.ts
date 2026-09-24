import path from 'node:path'

import { expect, jest, test } from '@jest/globals'

jest.unstable_mockModule('node:path', () => ({ ...path.win32, default: path.win32 }))

const { parseLocalScheme } = await import('../src/parseBareSpecifier.js')

const projectDir = 'C:\\repo\\deep\\a\\app'
const opts = { preserveAbsolutePaths: false }

test.each([
  ['file:vendor/library-a.tgz', 'file:vendor/library-a.tgz'],
  ['file:vendor\\library-a.tgz', 'file:vendor/library-a.tgz'],
  ['link:../../../bar', 'link:../../../bar'],
  ['link:..\\..\\..\\bar', 'link:../../../bar'],
])('the normalized specifier of %s uses forward slashes on Windows', (bareSpecifier, expected) => {
  expect(parseLocalScheme({ bareSpecifier }, projectDir, projectDir, opts)!.normalizedBareSpecifier).toBe(expected)
})
