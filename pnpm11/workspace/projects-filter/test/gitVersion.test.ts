import { expect, test } from '@jest/globals'

import { checkGitVersion, gitSupportsNoRelative, parseGitVersion } from '../src/gitVersion.js'

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

test('checkGitVersion rejects git older than 2.24', () => {
  expect(() => {
    checkGitVersion({ major: 2, minor: 17 })
  }).toThrow('Filtering by changed packages failed. The [<since>] selector requires git 2.24 or newer, but git 2.17 is installed.')
})

test('checkGitVersion accepts git 2.24, newer, and unknown versions', () => {
  expect(() => {
    checkGitVersion({ major: 2, minor: 24 })
    checkGitVersion({ major: 3, minor: 0 })
    checkGitVersion(undefined)
  }).not.toThrow()
})

test('gitSupportsNoRelative is true from git 2.28 and for unknown versions', () => {
  expect(gitSupportsNoRelative({ major: 2, minor: 27 })).toBe(false)
  expect(gitSupportsNoRelative({ major: 2, minor: 28 })).toBe(true)
  expect(gitSupportsNoRelative(undefined)).toBe(true)
})
