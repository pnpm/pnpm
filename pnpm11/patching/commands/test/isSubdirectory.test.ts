import path from 'node:path'

import { expect, test } from '@jest/globals'

import { isSubdirectory } from '../src/isSubdirectory.js'

test('isSubdirectory() accepts paths inside the parent directory', () => {
  expect(isSubdirectory('/project/patches', '/project/patches/pkg.patch')).toBe(true)
  expect(isSubdirectory('/project/patches', '/project/patches/..pkg/pkg.patch')).toBe(true)
})

test('isSubdirectory() rejects parent traversal and sibling prefixes', () => {
  expect(isSubdirectory('/project/patches', '/project/pkg.patch')).toBe(false)
  expect(isSubdirectory('/project/patches', '/project/patches-other/pkg.patch')).toBe(false)
})

test('isSubdirectory() rejects Windows drive and UNC escapes', () => {
  expect(isSubdirectory('C:\\project\\patches', 'D:\\pkg.patch', path.win32)).toBe(false)
  expect(isSubdirectory('C:\\project\\patches', '\\\\server\\share\\pkg.patch', path.win32)).toBe(false)
  expect(isSubdirectory('C:\\project\\patches', 'C:\\project\\patches\\..pkg\\pkg.patch', path.win32)).toBe(true)
})
