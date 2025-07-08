import path from 'path'
import { type Log } from '@pnpm/core-loggers'
import { requireHooks, BadReadPackageHookError, type HookContext } from '@pnpm/pnpmfile'
import { requirePnpmfile } from '../src/requirePnpmfile'

const defaultHookContext: HookContext = { log () {} }

test('ignoring a pnpmfile that exports undefined', async () => {
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(path.join(__dirname, '__fixtures__/undefined.js'), __dirname))!
  expect(pnpmfile).toBeUndefined()
})

test('readPackage hook run fails when returns undefined ', async () => {
  const pnpmfilePath = path.join(__dirname, '__fixtures__/readPackageNoReturn.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, __dirname))!

  return expect(
    pnpmfile!.hooks!.readPackage!({}, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook did not return a package manifest object.'))
})

test('readPackage hook run fails when returned dependencies is not an object ', async () => {
  const pnpmfilePath = path.join(__dirname, '__fixtures__/readPackageNoObject.js')
  const { pnpmfileModule: pnpmfile } = (await requirePnpmfile(pnpmfilePath, __dirname))!
  return expect(
    pnpmfile!.hooks!.readPackage!({}, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned package manifest object\'s property \'dependencies\' must be an object.'))
})

test('filterLog hook combines with the global hook', async () => {
  const globalPnpmfile = path.join(__dirname, '__fixtures__/globalFilterLog.js')
  const pnpmfile = path.join(__dirname, '__fixtures__/filterLog.js')
  const { hooks } = await requireHooks(__dirname, { globalPnpmfile, pnpmfile: [pnpmfile] })

  expect(hooks.filterLog).toBeDefined()
  expect(hooks.filterLog!.length).toBe(2)
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

test('calculatePnpmfileChecksum is undefined when pnpmfile does not exist', async () => {
  const { hooks } = await requireHooks(__dirname, {})
  expect(hooks.calculatePnpmfileChecksum).toBeUndefined()
})

test('calculatePnpmfileChecksum resolves to hash string for existing pnpmfile', async () => {
  const pnpmfile = path.join(__dirname, '__fixtures__/readPackageNoObject.js')
  const { hooks } = await requireHooks(__dirname, { pnpmfile: [pnpmfile] })
  expect(typeof await hooks.calculatePnpmfileChecksum?.()).toBe('string')
})

test('calculatePnpmfileChecksum is undefined if pnpmfile even when it exports undefined', async () => {
  const pnpmfile = path.join(__dirname, '__fixtures__/undefined.js')
  const { hooks } = await requireHooks(__dirname, { pnpmfile: [pnpmfile] })
  expect(hooks.calculatePnpmfileChecksum).toBeUndefined()
})

test('updateConfig throws an error if it returns undefined', async () => {
  const pnpmfile = path.join(__dirname, '__fixtures__/updateConfigReturnsUndefined.js')
  const { hooks } = await requireHooks(__dirname, { pnpmfile: [pnpmfile] })
  expect(() => hooks.updateConfig![0]!({})).toThrow('The updateConfig hook returned undefined')
})

test('requireHooks throw an error if one of the specified pnpmfiles does not exist', async () => {
  await expect(requireHooks(__dirname, { pnpmfile: ['does-not-exist.cjs'] })).rejects.toThrow('is not found')
})
