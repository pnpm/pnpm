import { PackageSelector, parsePackageSelector } from '@pnpm/filter-workspace-packages'
import path = require('path')
import isWindows = require('is-windows')

const fixtures: Array<[string, PackageSelector]> = [
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

test('parsePackageSelector()', () => {
  for (const fixture of fixtures) {
    expect(
      parsePackageSelector(fixture[0], process.cwd())).toStrictEqual(fixture[1])
  }
  if (isWindows()) {
    expect(
      parsePackageSelector('.\\foo', process.cwd())).toStrictEqual(
      { excludeSelf: false, parentDir: path.resolve('foo') }
    )
    expect(
      parsePackageSelector('..\\foo', process.cwd())).toStrictEqual(
      { excludeSelf: false, parentDir: path.resolve('../foo') }
    )
  }
})
