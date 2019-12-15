import { parsePackageSelector } from '@pnpm/filter-workspace-packages'
import isWindows = require('is-windows')
import path = require('path')
import test = require('tape')

test('parsePackageSelector()', (t) => {
  t.deepEqual(
    parsePackageSelector('foo', process.cwd()),
    { excludeSelf: false, pattern: 'foo', scope: 'exact', selectBy: 'name' },
  )
  t.deepEqual(
    parsePackageSelector('foo...', process.cwd()),
    { excludeSelf: false, pattern: 'foo', scope: 'dependencies', selectBy: 'name' },
  )
  t.deepEqual(
    parsePackageSelector('...foo', process.cwd()),
    { excludeSelf: false, pattern: 'foo', scope: 'dependents', selectBy: 'name' },
  )
  t.deepEqual(
    parsePackageSelector('foo^...', process.cwd()),
    { excludeSelf: true, pattern: 'foo', scope: 'dependencies', selectBy: 'name' },
  )
  t.deepEqual(
    parsePackageSelector('...^foo', process.cwd()),
    { excludeSelf: true, pattern: 'foo', scope: 'dependents', selectBy: 'name' },
  )
  t.deepEqual(
    parsePackageSelector('./foo', process.cwd()),
    { excludeSelf: false, pattern: path.resolve('foo'), scope: 'exact', selectBy: 'location' },
  )
  t.deepEqual(
    parsePackageSelector('../foo', process.cwd()),
    { excludeSelf: false, pattern: path.resolve('../foo'), scope: 'exact', selectBy: 'location' },
  )
  t.deepEqual(
    parsePackageSelector('.', process.cwd()),
    { excludeSelf: false, pattern: process.cwd(), scope: 'exact', selectBy: 'location' },
  )
  t.deepEqual(
    parsePackageSelector('..', process.cwd()),
    { excludeSelf: false, pattern: path.resolve('..'), scope: 'exact', selectBy: 'location' },
  )
  if (isWindows()) {
    t.deepEqual(
      parsePackageSelector('.\\foo', process.cwd()),
      { excludeSelf: false, pattern: path.resolve('foo'), scope: 'exact', selectBy: 'location' },
    )
    t.deepEqual(
      parsePackageSelector('..\\foo', process.cwd()),
      { excludeSelf: false, pattern: path.resolve('../foo'), scope: 'exact', selectBy: 'location' },
    )
  }
  t.end()
})
