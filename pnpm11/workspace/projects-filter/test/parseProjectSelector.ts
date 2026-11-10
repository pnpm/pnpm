import path from 'node:path'

import { expect, test } from '@jest/globals'
import { parseProjectSelector, type ProjectSelector, rebaseProjectSelector } from '@pnpm/workspace.projects-filter'
import isWindows from 'is-windows'

const fixtures: Array<[string, ProjectSelector]> = [
  [
    'foo',
    {
      diff: undefined,
      exclude: false,
      excludeSelf: false,
      includeDependencies: false,
      includeDependents: false,
      namePattern: 'foo',
      parentDir: undefined,
    },
  ],
  [
    'foo...',
    {
      diff: undefined,
      exclude: false,
      excludeSelf: false,
      includeDependencies: true,
      includeDependents: false,
      namePattern: 'foo',
      parentDir: undefined,
    },
  ],
  [
    '...foo',
    {
      diff: undefined,
      exclude: false,
      excludeSelf: false,
      includeDependencies: false,
      includeDependents: true,
      namePattern: 'foo',
      parentDir: undefined,
    },
  ],
  [
    '...foo...',
    {
      diff: undefined,
      exclude: false,
      excludeSelf: false,
      includeDependencies: true,
      includeDependents: true,
      namePattern: 'foo',
      parentDir: undefined,
    },
  ],
  [
    'foo^...',
    {
      diff: undefined,
      exclude: false,
      excludeSelf: true,
      includeDependencies: true,
      includeDependents: false,
      namePattern: 'foo',
      parentDir: undefined,
    },
  ],
  [
    '...^foo',
    {
      diff: undefined,
      exclude: false,
      excludeSelf: true,
      includeDependencies: false,
      includeDependents: true,
      namePattern: 'foo',
      parentDir: undefined,
    },
  ],
  [
    './foo',
    {
      exclude: false,
      excludeSelf: false,
      parentDir: path.resolve('foo'),
    },
  ],
  [
    '../foo',
    {
      exclude: false,
      excludeSelf: false,
      parentDir: path.resolve('../foo'),
    },
  ],
  [
    '...{./foo}',
    {
      diff: undefined,
      exclude: false,
      excludeSelf: false,
      includeDependencies: false,
      includeDependents: true,
      namePattern: undefined,
      parentDir: path.resolve('foo'),
    },
  ],
  [
    '.',
    {
      exclude: false,
      excludeSelf: false,
      parentDir: process.cwd(),
    },
  ],
  [
    '..',
    {
      exclude: false,
      excludeSelf: false,
      parentDir: path.resolve('..'),
    },
  ],
  [
    '[master]',
    {
      diff: 'master',
      exclude: false,
      excludeSelf: false,
      includeDependencies: false,
      includeDependents: false,
      namePattern: undefined,
      parentDir: undefined,
    },
  ],
  [
    '{foo}[master]',
    {
      diff: 'master',
      exclude: false,
      excludeSelf: false,
      includeDependencies: false,
      includeDependents: false,
      namePattern: undefined,
      parentDir: path.resolve('foo'),
    },
  ],
  [
    'pattern{foo}[master]',
    {
      diff: 'master',
      exclude: false,
      excludeSelf: false,
      includeDependencies: false,
      includeDependents: false,
      namePattern: 'pattern',
      parentDir: path.resolve('foo'),
    },
  ],
  [
    '[master]...',
    {
      diff: 'master',
      exclude: false,
      excludeSelf: false,
      includeDependencies: true,
      includeDependents: false,
      namePattern: undefined,
      parentDir: undefined,
    },
  ],
  [
    '...[master]',
    {
      diff: 'master',
      exclude: false,
      excludeSelf: false,
      includeDependencies: false,
      includeDependents: true,
      namePattern: undefined,
      parentDir: undefined,
    },
  ],
  [
    '...[master]...',
    {
      diff: 'master',
      exclude: false,
      excludeSelf: false,
      includeDependencies: true,
      includeDependents: true,
      namePattern: undefined,
      parentDir: undefined,
    },
  ],
]

test('parseProjectSelector()', () => {
  for (const fixture of fixtures) {
    expect(
      parseProjectSelector(fixture[0], process.cwd())).toStrictEqual(fixture[1])
  }
  if (isWindows()) {
    expect(
      parseProjectSelector('.\\foo', process.cwd())).toStrictEqual(
      { exclude: false, excludeSelf: false, parentDir: path.resolve('foo') }
    )
    expect(
      parseProjectSelector('..\\foo', process.cwd())).toStrictEqual(
      { exclude: false, excludeSelf: false, parentDir: path.resolve('../foo') }
    )
  }
})

test('rebaseProjectSelector()', () => {
  const from = path.resolve('/ws')
  const to = path.resolve('/ws/packages/other')
  const same = path.resolve('/ws/packages/app')
  for (const raw of [
    './packages/app',
    '!./packages/app',
    '.',
    '{packages/*}',
    '...^pattern{./packages/app}[main]...',
    'app',
    '[main]',
  ]) {
    const rebased = rebaseProjectSelector(raw, from, to)
    expect(parseProjectSelector(rebased, to)).toStrictEqual(parseProjectSelector(raw, from))
    const rebasedSame = rebaseProjectSelector(raw, from, same)
    expect(parseProjectSelector(rebasedSame, same)).toStrictEqual(parseProjectSelector(raw, from))
  }
  expect(rebaseProjectSelector('./packages/app', from, to)).toBe('../app')
  expect(rebaseProjectSelector('./packages/app', from, same)).toBe('.')
  expect(rebaseProjectSelector('./packages/app/sub', from, same)).toBe('./sub')
})
