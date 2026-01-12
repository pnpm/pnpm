/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { readPackageJson, readPackageJsonFromDir } from '@pnpm/read-package-json'

const fixtures = path.join(import.meta.dirname, 'fixtures')

test('readPackageJson()', async () => {
  expect((await readPackageJson(path.join(import.meta.dirname, '..', 'package.json'))).name).toBe('@pnpm/read-package-json')
})

test('fromDir()', async () => {
  expect((await readPackageJsonFromDir(path.join(import.meta.dirname, '..'))).name).toBe('@pnpm/read-package-json')
})

test('readPackageJson() throw error when name is invalid', async () => {
  let err
  try {
    await readPackageJson(path.join(fixtures, 'invalid-name', 'package.json'))
  } catch (_: any) { // eslint-disable-line
    err = _
  }
  expect(err.code).toBe('ERR_PNPM_BAD_PACKAGE_JSON')
})

test('readPackageJson() throw initial error when package.json not found', async () => {
  let err
  try {
    await readPackageJson(path.join(fixtures, 'package.json'))
  } catch (_: any) { // eslint-disable-line
    err = _
  }
  expect(err.code).toBe('ENOENT')
})
