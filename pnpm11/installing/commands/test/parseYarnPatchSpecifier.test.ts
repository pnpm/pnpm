import { expect, test } from '@jest/globals'

import { parseYarnPatchSpecifier } from '../src/import/yarnPatches.js'

test('parses a yarn 4 patch of a registry version', () => {
  expect(parseYarnPatchSpecifier('patch:jest-runtime@npm%3A29.7.0#~/.yarn/patches/jest-runtime-npm-29.7.0-120fa64128.patch')).toStrictEqual({
    specifier: '29.7.0',
    patchKey: 'jest-runtime@29.7.0',
    patchPaths: ['~/.yarn/patches/jest-runtime-npm-29.7.0-120fa64128.patch'],
  })
})

test('parses a scoped patch with yarn 3 parameters', () => {
  expect(parseYarnPatchSpecifier('patch:@scope/pkg@npm%3A%5E1.2.0#./.yarn/patches/pkg.patch::version=1.2.3&hash=abc&locator=root%40workspace%3A.')).toStrictEqual({
    specifier: '^1.2.0',
    patchKey: '@scope/pkg@^1.2.0',
    patchPaths: ['./.yarn/patches/pkg.patch'],
  })
})

test('keys an aliased patch by the real package', () => {
  expect(parseYarnPatchSpecifier('patch:foo@npm%3A@scope/bar@1.0.0#~/foo.patch')).toStrictEqual({
    specifier: 'npm:@scope/bar@1.0.0',
    patchKey: '@scope/bar@1.0.0',
    patchPaths: ['~/foo.patch'],
  })
})

test('keys a non-registry patch by name', () => {
  expect(parseYarnPatchSpecifier('patch:foo@https%3A//example.com/foo.tgz#~/foo.patch')).toStrictEqual({
    specifier: 'https://example.com/foo.tgz',
    patchKey: 'foo',
    patchPaths: ['~/foo.patch'],
  })
})

test('splits several patches and skips yarn builtins', () => {
  expect(parseYarnPatchSpecifier('patch:typescript@npm%3A5.0.0#optional!builtin<compat/typescript>&./a.patch&./b.patch')?.patchPaths)
    .toStrictEqual(['./a.patch', './b.patch'])
  expect(parseYarnPatchSpecifier('patch:resolve@npm%3A1.22.0#builtin<compat/resolve>')?.patchPaths).toStrictEqual([])
})

test('ignores other protocols', () => {
  expect(parseYarnPatchSpecifier('npm:foo@1.0.0')).toBeUndefined()
  expect(parseYarnPatchSpecifier('^1.0.0')).toBeUndefined()
})
