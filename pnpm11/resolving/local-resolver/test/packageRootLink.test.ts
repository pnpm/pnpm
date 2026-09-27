import { expect, test } from '@jest/globals'
import { fileSpecToPackageRootLink } from '@pnpm/resolving.local-resolver'

test.each([
  ['file:./typings/css-tree', 'link:<root>/typings/css-tree'],
  ['file:typings/css-tree', 'link:<root>/typings/css-tree'],
  ['file:./a/../b/', 'link:<root>/b'],
  ['file:.\\typings\\css-tree', 'link:<root>/typings/css-tree'],
])('fileSpecToPackageRootLink(%s) links inside the package', (bareSpecifier, expected) => {
  expect(fileSpecToPackageRootLink(bareSpecifier)).toBe(expected)
})

test.each([
  'file:.',
  'file:./',
  'file:',
  'file:../sibling',
  'file:./a/../../sibling',
  'file:/abs/path',
  'file:C:\\abs\\path',
  'file:~/dir',
  'file:./vendor/pkg-1.0.0.tgz',
  'file:child/C:/Users/Public',
  'link:./child',
  '^1.0.0',
])('fileSpecToPackageRootLink(%s) leaves the specifier alone', (bareSpecifier) => {
  expect(fileSpecToPackageRootLink(bareSpecifier)).toBeUndefined()
})
