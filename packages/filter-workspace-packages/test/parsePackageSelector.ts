import { PackageSelector, parsePackageSelector } from '@pnpm/filter-workspace-packages'
import isWindows = require('is-windows')
import path = require('path')
import test = require('tape')

const fixtures: Array<[string, PackageSelector]> = [
  [
    'foo',
    {
      diff: undefined,
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
      excludeSelf: false,
      parentDir: path.resolve('foo'),
    },
  ],
  [
    '../foo',
    {
      excludeSelf: false,
      parentDir: path.resolve('../foo'),
    },
  ],
  [
    '...{./foo}',
    {
      diff: undefined,
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
      excludeSelf: false,
      parentDir: process.cwd(),
    },
  ],
  [
    '..',
    {
      excludeSelf: false,
      parentDir: path.resolve('..'),
    },
  ],
  [
    '[master]',
    {
      diff: 'master',
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
      excludeSelf: false,
      includeDependencies: true,
      includeDependents: true,
      namePattern: undefined,
      parentDir: undefined,
    },
  ],
]

test('parsePackageSelector()', (t) => {
  for (let fixture of fixtures) {
    t.deepEqual(
      parsePackageSelector(fixture[0], process.cwd()),
      fixture[1],
      `parsing ${fixture[0]}`
    )
  }
  if (isWindows()) {
    t.deepEqual(
      parsePackageSelector('.\\foo', process.cwd()),
      { excludeSelf: false, parentDir: path.resolve('foo') }
    )
    t.deepEqual(
      parsePackageSelector('..\\foo', process.cwd()),
      { excludeSelf: false, parentDir: path.resolve('../foo') }
    )
  }
  t.end()
})
