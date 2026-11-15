import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

import { expect, test } from '@jest/globals'
import type { Log } from '@pnpm/core-loggers'
import { BadReadPackageHookError, type HookContext, requireHooks } from '@pnpm/hooks.pnpmfile'
import { fixtures } from '@pnpm/test-fixtures'

import { requirePnpmfile } from '../src/requirePnpmfile.js'

const defaultHookContext: HookContext = { log () {} }
const testFixtures = fixtures(import.meta.dirname)

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

test('readPackage hook run fails when it returns a non-object value', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/readPackageReturnsString.js')
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

test('readPackage hook run fails when it sets a dependency range to undefined', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/readPackageUndefinedRange.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, import.meta.dirname))!
  return expect(
    pnpmfile!.hooks!.readPackage!({ name: 'foo', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } }, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned an invalid range for \'is-positive\' in the \'dependencies\' of foo@1.0.0. Expected a string, got undefined. To remove the dependency, delete the property.'))
})

test('readPackage hook run fails when it sets a peer dependency range to a number', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/readPackageNumberPeerRange.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, import.meta.dirname))!
  return expect(
    pnpmfile!.hooks!.readPackage!({ name: 'foo' }, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned an invalid range for \'is-positive\' in the \'peerDependencies\' of foo. Expected a string, got number. To remove the dependency, delete the property.'))
})

test.each([
  { pkg: { name: Symbol('foo') }, described: 'an unnamed package' },
  { pkg: { name: 'foo', version: Object.create(null) }, described: 'foo' },
])('readPackage hook run reports an invalid range of a package whose identity is not a string', async ({ pkg, described }) => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/readPackageNumberPeerRange.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, import.meta.dirname))!
  return expect(
    pnpmfile!.hooks!.readPackage!(pkg, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, `readPackage hook returned an invalid range for 'is-positive' in the 'peerDependencies' of ${described}. Expected a string, got number. To remove the dependency, delete the property.`))
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

test('falls back to .pnpmfile.cjs when .pnpmfile.mjs does not exist', async () => {
  const fixtureDir = path.join(import.meta.dirname, '__fixtures__/default-cjs-only')
  const { hooks, resolvedPnpmfilePaths } = await requireHooks(fixtureDir, { tryLoadDefaultPnpmfile: true })

  expect(hooks.readPackage).toHaveLength(0)
  expect(resolvedPnpmfilePaths).toStrictEqual([path.join(fixtureDir, '.pnpmfile.cjs')])
})

test.each(['default', 'default-esm'])('the project pnpmfile stays in the checksum when it is also the global pnpmfile (%s)', async (fixture) => {
  const fixtureDir = path.join(import.meta.dirname, '__fixtures__', fixture)
  const globalPnpmfile = path.join(fixtureDir, fixture === 'default' ? '.pnpmfile.cjs' : '.pnpmfile.mjs')
  const { hooks: projectOnly } = await requireHooks(fixtureDir, { tryLoadDefaultPnpmfile: true })
  const { hooks: both, resolvedPnpmfilePaths } = await requireHooks(fixtureDir, { globalPnpmfile, tryLoadDefaultPnpmfile: true })

  expect(resolvedPnpmfilePaths).toStrictEqual([globalPnpmfile])
  expect(both.untrackedPnpmfileReadPackageHook).not.toBe(true)
  expect(await both.calculatePnpmfileChecksum!()).toBe(await projectOnly.calculatePnpmfileChecksum!())
})

test('calculatePnpmfileChecksum is undefined when pnpmfile does not exist', async () => {
  const { hooks } = await requireHooks(import.meta.dirname, {})
  expect(hooks.calculatePnpmfileChecksum).toBeUndefined()
})

test('ignores a missing default pnpmfile when asynchronous module hooks are registered', () => {
  const fixtureDir = path.join(import.meta.dirname, '__fixtures__/async-loader')
  const registerLoader = pathToFileURL(path.join(fixtureDir, 'register-loader.mjs')).href
  execFileSync(process.execPath, [path.join(fixtureDir, 'load-missing-default-pnpmfile.mjs')], {
    env: {
      ...process.env,
      NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ''} --disable-warning=DEP0205 --import=${registerLoader}`.trim(),
    },
  })
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

test('updateConfig throws an error if it resolves to undefined', async () => {
  const pnpmfile = path.join(import.meta.dirname, '__fixtures__/updateConfigResolvesUndefined.js')
  const { hooks } = await requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile] })
  await expect(hooks.updateConfig![0]!({})).rejects.toThrow('The updateConfig hook returned undefined')
})

test('afterAllResolved throws BAD_AFTER_ALL_RESOLVED_RETURN if it returns or resolves to undefined or non-object', async () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpmfile-after-all-resolved-'))
  const lockfile = { lockfileVersion: '9.0', importers: {}, packages: {} } as never
  try {
    const syncUndefined = path.join(tempDir, 'sync-undef.cjs')
    fs.writeFileSync(syncUndefined, 'module.exports = { hooks: { afterAllResolved () {} } }')
    const { hooks: syncHooks } = await requireHooks(tempDir, { pnpmfiles: [syncUndefined] })
    expect(() => syncHooks.afterAllResolved![0]!(lockfile)).toThrow(
      expect.objectContaining({
        code: 'ERR_PNPM_BAD_AFTER_ALL_RESOLVED_RETURN',
        message: 'The afterAllResolved hook returned undefined',
      })
    )

    const asyncUndefined = path.join(tempDir, 'async-undef.cjs')
    fs.writeFileSync(asyncUndefined, 'module.exports = { hooks: { async afterAllResolved () { return null } } }')
    const { hooks: asyncHooks } = await requireHooks(tempDir, { pnpmfiles: [asyncUndefined] })
    await expect(asyncHooks.afterAllResolved![0]!(lockfile)).rejects.toMatchObject({
      code: 'ERR_PNPM_BAD_AFTER_ALL_RESOLVED_RETURN',
      message: 'The afterAllResolved hook returned undefined',
    })

    const nonObj = path.join(tempDir, 'non-obj.cjs')
    fs.writeFileSync(nonObj, 'module.exports = { hooks: { afterAllResolved () { return [] } } }')
    const { hooks: nonObjHooks } = await requireHooks(tempDir, { pnpmfiles: [nonObj] })
    expect(() => nonObjHooks.afterAllResolved![0]!(lockfile)).toThrow(
      expect.objectContaining({
        code: 'ERR_PNPM_BAD_AFTER_ALL_RESOLVED_RETURN',
        message: 'The afterAllResolved hook did not return a lockfile object',
      })
    )

    const badConfigNull = path.join(tempDir, 'config-null.cjs')
    fs.writeFileSync(badConfigNull, 'module.exports = { hooks: { updateConfig () { return null } } }')
    const { hooks: configNullHooks } = await requireHooks(tempDir, { pnpmfiles: [badConfigNull] })
    expect(() => configNullHooks.updateConfig![0]!({} as never)).toThrow(
      expect.objectContaining({
        code: 'ERR_PNPM_CONFIG_IS_UNDEFINED',
        message: 'The updateConfig hook returned undefined',
      })
    )

    const badConfigArr = path.join(tempDir, 'config-arr.cjs')
    fs.writeFileSync(badConfigArr, 'module.exports = { hooks: { async updateConfig () { return [] } } }')
    const { hooks: configArrHooks } = await requireHooks(tempDir, { pnpmfiles: [badConfigArr] })
    await expect(configArrHooks.updateConfig![0]!({} as never)).rejects.toMatchObject({
      code: 'ERR_PNPM_CONFIG_IS_UNDEFINED',
      message: 'The updateConfig hook did not return a config object',
    })
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true })
  }
})

test('requirePnpmfile wraps non-native-Error throws instead of crashing', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/throwsString.cjs')
  await expect(requirePnpmfile(pnpmfilePath, import.meta.dirname)).rejects.toThrow('this is a string error, not a native Error')
})

test('requirePnpmfile reports missing imports from an existing ESM pnpmfile', async () => {
  const pnpmfilePath = path.join(import.meta.dirname, '__fixtures__/missing-import/.pnpmfile.mjs')
  await expect(requirePnpmfile(pnpmfilePath, import.meta.dirname)).rejects.toMatchObject({
    code: 'ERR_PNPM_PNPMFILE_FAIL',
    message: expect.stringContaining('Error during pnpmfile execution'),
  })
})

test.each(['cjs', 'mjs'])('requireHooks throws an error if a specified %s pnpmfile does not exist', async (extension) => {
  await expect(requireHooks(import.meta.dirname, { pnpmfiles: [`does-not-exist.${extension}`] })).rejects.toMatchObject({
    code: 'ERR_PNPM_PNPMFILE_NOT_FOUND',
    message: expect.stringContaining('is not found'),
  })
})

test('requireHooks throws an error if there are two finders with the same name', async () => {
  const findersDir = testFixtures.find('finders')
  const pnpmfile1 = path.join(findersDir, 'finderFoo1.js')
  const pnpmfile2 = path.join(findersDir, 'finderFoo2.js')
  await expect(requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile1, pnpmfile2] })).rejects.toThrow('Finder "foo" defined in both')
})

test('requireHooks merges all the finders', async () => {
  const findersDir = testFixtures.find('finders')
  const pnpmfile1 = path.join(findersDir, 'finderFoo1.js')
  const pnpmfile2 = path.join(findersDir, 'finderBar.js')
  const { finders } = await requireHooks(import.meta.dirname, { pnpmfiles: [pnpmfile1, pnpmfile2] })
  expect(finders.foo).toBeDefined()
  expect(finders.bar).toBeDefined()
})
