import { parsePackageSelector } from '@pnpm/filter-workspace-packages'
import isWindows = require('is-windows')
import path = require('path')
import test = require('tape')

test('parsePackageSelector()', (t) => {
  t.deepEqual(
    parsePackageSelector('foo', process.cwd()),
    { excludeSelf: false, namePattern: 'foo' },
  )
  t.deepEqual(
    parsePackageSelector('foo...', process.cwd()),
    { excludeSelf: false, namePattern: 'foo', includeDependencies: true, includeDependents: false },
  )
  t.deepEqual(
    parsePackageSelector('...foo', process.cwd()),
    { excludeSelf: false, namePattern: 'foo', includeDependencies: false, includeDependents: true },
  )
  t.deepEqual(
    parsePackageSelector('...foo...', process.cwd()),
    { excludeSelf: false, namePattern: 'foo', includeDependencies: true, includeDependents: true },
  )
  t.deepEqual(
    parsePackageSelector('foo^...', process.cwd()),
    { excludeSelf: true, namePattern: 'foo', includeDependencies: true, includeDependents: false },
  )
  t.deepEqual(
    parsePackageSelector('...^foo', process.cwd()),
    { excludeSelf: true, namePattern: 'foo', includeDependencies: false, includeDependents: true },
  )
  t.deepEqual(
    parsePackageSelector('./foo', process.cwd()),
    { excludeSelf: false, parentDir: path.resolve('foo') },
  )
  t.deepEqual(
    parsePackageSelector('../foo', process.cwd()),
    { excludeSelf: false, parentDir: path.resolve('../foo') },
  )
  t.deepEqual(
    parsePackageSelector('.', process.cwd()),
    { excludeSelf: false, parentDir: process.cwd() },
  )
  t.deepEqual(
    parsePackageSelector('..', process.cwd()),
    { excludeSelf: false, parentDir: path.resolve('..') },
  )
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
