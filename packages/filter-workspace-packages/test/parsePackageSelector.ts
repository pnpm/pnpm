import { PackageSelector, parsePackageSelector } from '@pnpm/filter-workspace-packages'
import isWindows = require('is-windows')
import path = require('path')
import test = require('tape')

const fixtures: Array<[string, PackageSelector]> = [
  [
    'foo',
    { excludeSelf: false, namePattern: 'foo' },
  ],
  [
    'foo...',
    { excludeSelf: false, namePattern: 'foo', includeDependencies: true, includeDependents: false },
  ],
  [
    '...foo',
    { excludeSelf: false, namePattern: 'foo', includeDependencies: false, includeDependents: true },
  ],
  [
    '...foo...',
    { excludeSelf: false, namePattern: 'foo', includeDependencies: true, includeDependents: true },
  ],
  [
    'foo^...',
    { excludeSelf: true, namePattern: 'foo', includeDependencies: true, includeDependents: false },
  ],
  [
    '...^foo',
    { excludeSelf: true, namePattern: 'foo', includeDependencies: false, includeDependents: true },
  ],
  [
    './foo',
    { excludeSelf: false, parentDir: path.resolve('foo') },
  ],
  [
    '../foo',
    { excludeSelf: false, parentDir: path.resolve('../foo') },
  ],
  [
    '.',
    { excludeSelf: false, parentDir: process.cwd() },
  ],
  [
    '..',
    { excludeSelf: false, parentDir: path.resolve('..') },
  ],
  [
    '[master]',
    { diff: 'master', excludeSelf: false, includeDependencies: false, includeDependents: false },
  ],
  [
    '[master]...',
    { diff: 'master', excludeSelf: false, includeDependencies: true, includeDependents: false },
  ],
  [
    '...[master]',
    { diff: 'master', excludeSelf: false, includeDependencies: false, includeDependents: true },
  ],
  [
    '...[master]...',
    { diff: 'master', excludeSelf: false, includeDependencies: true, includeDependents: true },
  ],
]

test('parsePackageSelector()', (t) => {
  for (let fixture of fixtures) {
    t.deepEqual(
      parsePackageSelector(fixture[0], process.cwd()),
      fixture[1],
      `parsing ${fixture[0]}`,
    )
  }
  if (isWindows()) {
    t.deepEqual(
      parsePackageSelector('.\\foo', process.cwd()),
      { excludeSelf: false, parentDir: path.resolve('foo') },
    )
    t.deepEqual(
      parsePackageSelector('..\\foo', process.cwd()),
      { excludeSelf: false, parentDir: path.resolve('../foo') },
    )
  }
  t.end()
})
