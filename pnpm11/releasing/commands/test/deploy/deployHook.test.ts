import { expect, test } from '@jest/globals'

import { deployHook } from '../../src/deploy/deployHook.js'

test('deployHook()', () => {
  expect(deployHook({
    dependencies: {
      a: 'workspace:1',
    },
    devDependencies: {
      b: 'workspace:2',
    },
    optionalDependencies: {
      c: 'workspace:3',
    },
  })).toStrictEqual({
    dependencies: {
      a: 'workspace:1',
    },
    devDependencies: {
      b: 'workspace:2',
    },
    optionalDependencies: {
      c: 'workspace:3',
    },
    dependenciesMeta: {
      a: {
        injected: true,
      },
      b: {
        injected: true,
      },
      c: {
        injected: true,
      },
    },
  })
})

test('deployHook copies linked dependencies from every dependency field', () => {
  expect(deployHook({
    dependencies: { a: 'link:./a', registry: '^1.0.0', file: 'file:./file' },
    devDependencies: { b: 'link:../b' },
    optionalDependencies: { c: 'link:./c' },
  }, { convertLinksToFileProtocol: true })).toStrictEqual({
    dependencies: { a: 'file:./a', registry: '^1.0.0', file: 'file:./file' },
    devDependencies: { b: 'file:../b' },
    optionalDependencies: { c: 'file:./c' },
    dependenciesMeta: {},
  })
})

test('deployHook preserves links when installing a shared deploy lockfile', () => {
  expect(deployHook({ dependencies: { self: 'link:.' } })).toStrictEqual({
    dependencies: { self: 'link:.' },
    dependenciesMeta: {},
  })
})
