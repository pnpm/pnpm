/// <reference path="../../../typings/index.d.ts"/>
import readPackageJson, { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import path = require('path')
import test = require('tape')

const fixtures = path.join(__dirname, 'fixtures')

test('readPackageJson()', async (t) => {
  t.equal((await readPackageJson(path.join(__dirname, '..', 'package.json'))).name, '@pnpm/read-package-json')
  t.end()
})

test('fromDir()', async (t) => {
  t.equal((await readPackageJsonFromDir(path.join(__dirname, '..'))).name, '@pnpm/read-package-json')
  t.end()
})

test('readPackageJson() throw error when name is invalid', async (t) => {
  let err
  try {
    await readPackageJson(path.join(fixtures, 'invalid-name', 'package.json'))
  } catch (_) {
    err = _
  }
  t.equal(err.code, 'ERR_PNPM_BAD_PACKAGE_JSON')
  t.end()
})

test('readPackageJson() throw initial error when package.json not found', async (t) => {
  let err
  try {
    await readPackageJson(path.join(fixtures, 'package.json'))
  } catch (_) {
    err = _
  }
  t.equal(err.code, 'ENOENT')
  t.end()
})
