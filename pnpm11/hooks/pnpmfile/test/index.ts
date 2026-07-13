import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

import { expect, jest, test } from '@jest/globals'
import type { Log } from '@pnpm/core-loggers'
import { BadReadPackageHookError, type HookContext, requireHooks } from '@pnpm/hooks.pnpmfile'
import { fixtures } from '@pnpm/test-fixtures'

import { requirePnpmfile } from '../src/requirePnpmfile.js'

const defaultHookContext: HookContext = { log () {} }
const f = fixtures(import.meta.dirname)

test('ignoring a pnpmfile that exports undefined', async () => {
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(path.join(import.meta.dirname, '__fixtures__/undefined.js'), import.meta.dirname))!
  expect(pnpmfile).toBeUndefined()
})

test('readPackage hook run fails when returns undefined', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/readPackageNoReturn.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, import.meta.dirname))!

  return expect(
    pnpmfile!.hooks!.readPackage!({}, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook did not return a package manifest object.'))
})

test('readPackage hook run fails when returned dependencies is not an object', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/readPackageNoObject.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, import.meta.dirname))!
  return expect(
    pnpmfile!.hooks!.readPackage!({}, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned package manifest object\'s property \'dependencies\' must be an object.'))
})

test('readPackage hook run fails when returned devDependencies is not an object', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/readPackageNoObjectDevDependencies.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, import.meta.dirname))!
  return expect(
    pnpmfile!.hooks!.readPackage!({}, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned package manifest object\'s property \'devDependencies\' must be an object.'))
})

test('readPackage hook run fails when returned optionalDependencies is a falsy non-object value', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/readPackageFalsyOptionalDependencies.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, import.meta.dirname))!
  return expect(
    pnpmfile!.hooks!.readPackage!({}, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned package manifest object\'s property \'optionalDependencies\' must be an object.'))
})

test('readPackage hook run fails when returned peerDependencies is an array', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/readPackageArrayPeerDependencies.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, import.meta.dirname))!
  return expect(
    pnpmfile!.hooks!.readPackage!({}, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned package manifest object\'s property \'peerDependencies\' must be an object.'))
})

test('filterLog hook combines with the global hook', async () => {
  const globalPnpmfile = path.join(import.meta.dirname, '__fixtures__/globalFilterLog.js')
  const pnpmfile = path.join(import.meta.dirname, '__fixtures__/filterLog.js')
  const { hooks } = await requireHooks(import.meta.dirname, { globalPnpmfile, pnpmfiles: [pnpmfile] })

  expect(hooks.filterLog).toBeDefined()
  expect(hooks.filterLog!).toHaveLength(2)
  const filterLog = (log: Log) => hooks.filterLog!.every((hook) => hook(log))
  expect(filterLog({
    name: 'pnpm:summary',
    level: 'error',
    prefix: 'test',
  })).toBeTruthy()
  expect(filterLog({
    name: 'pnpm:summary',
    level: 'debug',
    prefix: 'test',
  })).toBeFalsy()
})

test('ignoring the default pnpmfile if tryLoadDefaultPnpmfile is not set', async () => {
  const { hooks } = await requireHooks(path.join(import.meta.dirname, '__fixtures__/default'), {})
  expect(hooks.readPackage?.length).toBe(0)
})

test('loading the default pnpmfile if tryLoadDefaultPnpmfile is set to true', async () => {
  const { hooks } = await requireHooks(path.join(import.meta.dirname, '__fixtures__/default'), { tryLoadDefaultPnpmfile: true })
  expect(hooks.readPackage?.length).toBe(1)
})

test('loading the default .pnpmfile.mjs if tryLoadDefaultPnpmfile is set to true', async () => {
  const { hooks } = await requireHooks(path.join(import.meta.dirname, '__fixtures__/default-esm'), { tryLoadDefaultPnpmfile: true })
  expect(hooks.readPackage?.length).toBe(1)
})

test('.pnpmfile.mjs takes priority over .pnpmfile.cjs when both exist', async () => {
  const { hooks } = await requireHooks(path.join(import.meta.dirname, '__fixtures__/default-both'), { tryLoadDefaultPnpmfile: true })
  expect(hooks.readPackage?.length).toBe(1)
  const pkg: any = await hooks.readPackage![0]({ name: 'test', version: '1.0.0' }) // eslint-disable-line
  expect(pkg._fromMjs).toBe(true)
  expect(pkg._fromCjs).toBeUndefined()
})

test('falling back to .pnpmfile.cjs without classifying a missing default .pnpmfile.mjs import error', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-pnpmfile-'))
  const cjsPath = path.join(dir, '.pnpmfile.cjs')
  fs.writeFileSync(cjsPath, 'module.exports = { hooks: {} }\n')
  const isNativeErrorSpy = jest.spyOn(util.types, 'isNativeError').mockReturnValue(false)

  try {
    const { hooks, resolvedPnpmfilePaths } = await requireHooks(dir, { tryLoadDefaultPnpmfile: true })

    expect(hooks.readPackage?.length).toBe(0)
    expect(resolvedPnpmfilePaths).toStrictEqual([cjsPath])
    expect(isNativeErrorSpy).not.toHaveBeenCalled()
  } finally {
    isNativeErrorSpy.mockRestore()
    fs.rmSync(dir, { recursive: true })
  }
})

test('skipping missing default pnpmfiles', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-pnpmfile-'))

  try {
    const { hooks, resolvedPnpmfilePaths } = await requireHooks(dir, { tryLoadDefaultPnpmfile: true })

    expect(hooks.readPackage?.length).toBe(0)
    expect(resolvedPnpmfilePaths).toStrictEqual([])
  } finally {
    fs.rmSync(dir, { recursive: true })
  }
})

test('calculatePnpmfileChecksum is undefined when pnpmfile does not exist', async () => {
  const { hooks } = await requireHooks(import.meta.dirname, {})
  expect(hooks.calculatePnpmfileChecksum).toBeUndefined()
})

test('calculatePnpmfileChecksum resolves to hash string for existing pnpmfile', async () => {
  const pnpmfile = path.join(import.meta.dirname, '__fixtures__/readPackageNoObject.js')
  const { hooks } = await requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile] })
  expect(typeof await hooks.calculatePnpmfileChecksum?.()).toBe('string')
})

test('calculatePnpmfileChecksum is undefined if pnpmfile even when it exports undefined', async () => {
  const pnpmfile = path.join(import.meta.dirname, '__fixtures__/undefined.js')
  const { hooks } = await requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile] })
  expect(hooks.calculatePnpmfileChecksum).toBeUndefined()
})

test('updateConfig throws an error if it returns undefined', async () => {
  const pnpmfile = path.join(import.meta.dirname, '__fixtures__/updateConfigReturnsUndefined.js')
  const { hooks } = await requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile] })
  expect(() => hooks.updateConfig![0]!({})).toThrow('The updateConfig hook returned undefined')
})

test('requirePnpmfile wraps non-native-Error throws instead of crashing', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/throwsString.cjs')
  await expect(requirePnpmfile(pnpmfilePath, import.meta.dirname)).rejects.toThrow('this is a string error, not a native Error')
})

test('requirePnpmfile reports missing imports from an existing ESM pnpmfile', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-pnpmfile-'))
  const pnpmfilePath = path.join(dir, '.pnpmfile.mjs')
  fs.writeFileSync(pnpmfilePath, "import './missing.mjs'\n")

  try {
    await expect(requirePnpmfile(pnpmfilePath, dir)).rejects.toMatchObject({
      code: 'ERR_PNPM_PNPMFILE_FAIL',
      message: expect.stringContaining('Error during pnpmfile execution'),
    })
  } finally {
    fs.rmSync(dir, { recursive: true })
  }
})

test.each(['cjs', 'mjs'])('requireHooks throws an error if a specified %s pnpmfile does not exist', async (extension) => {
  await expect(requireHooks(import.meta.dirname, { pnpmfiles: [`does-not-exist.${extension}`] })).rejects.toMatchObject({
    code: 'ERR_PNPM_PNPMFILE_NOT_FOUND',
    message: expect.stringContaining('is not found'),
  })
})

test('requireHooks throws an error if there are two finders with the same name', async () => {
  const findersDir = f.find('finders')
  const pnpmfile1 = path.join(findersDir, 'finderFoo1.js')
  const pnpmfile2 = path.join(findersDir, 'finderFoo2.js')
  await expect(requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile1, pnpmfile2] })).rejects.toThrow('Finder "foo" defined in both')
})

test('requireHooks merges all the finders', async () => {
  const findersDir = f.find('finders')
  const pnpmfile1 = path.join(findersDir, 'finderFoo1.js')
  const pnpmfile2 = path.join(findersDir, 'finderBar.js')
  const { finders } = await requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile1, pnpmfile2] })
  expect(finders.foo).toBeDefined()
  expect(finders.bar).toBeDefined()
})
