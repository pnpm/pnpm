import { expect, test } from '@jest/globals'

import { parseGitVersion } from '../src/gitVersion.js'

test('parseGitVersion reads the major and minor version', () => {
  expect(parseGitVersion('git version 2.25.1\n')).toStrictEqual({ major: 2, minor: 25 })
  expect(parseGitVersion('git version 2.39.3 (Apple Git-145)\n')).toStrictEqual({ major: 2, minor: 39 })
  expect(parseGitVersion('git version 2.45.1.windows.1\n')).toStrictEqual({ major: 2, minor: 45 })
})

test('parseGitVersion rejects unknown output', () => {
  expect(parseGitVersion('')).toBeUndefined()
  expect(parseGitVersion('hub version 2.14.2\n')).toBeUndefined()
  expect(parseGitVersion('git version unknown\n')).toBeUndefined()
})
