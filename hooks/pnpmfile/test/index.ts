import path from 'path'
import { type Log } from '@pnpm/core-loggers'
import { requireHooks, BadReadPackageHookError, type HookContext } from '@pnpm/pnpmfile'
import { requirePnpmfile } from '../src/requirePnpmfile'

const defaultHookContext: HookContext = { log () {} }

test('ignoring a pnpmfile that exports undefined', () => {
  const { pnpmfileModule: pnpmfile } = requirePnpmfile(path.join(__dirname, '__fixtures__/undefined.js'), __dirname)!
  expect(pnpmfile).toBeUndefined()
})

test('readPackage hook run fails when returns undefined ', () => {
  const pnpmfilePath = path.join(__dirname, '__fixtures__/readPackageNoReturn.js')
  const { pnpmfileModule: pnpmfile } = requirePnpmfile(pnpmfilePath, __dirname)!

  return expect(
    pnpmfile!.hooks!.readPackage!({}, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook did not return a package manifest object.'))
})

test('readPackage hook run fails when returned dependencies is not an object ', () => {
  const pnpmfilePath = path.join(__dirname, '__fixtures__/readPackageNoObject.js')
  const { pnpmfileModule: pnpmfile } = requirePnpmfile(pnpmfilePath, __dirname)!
  return expect(
    pnpmfile!.hooks!.readPackage!({}, defaultHookContext)
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned package manifest object\'s property \'dependencies\' must be an object.'))
})

test('filterLog hook combines with the global hook', () => {
  const globalPnpmfile = path.join(__dirname, '__fixtures__/globalFilterLog.js')
  const pnpmfile = path.join(__dirname, '__fixtures__/filterLog.js')
  const { hooks } = requireHooks(__dirname, { globalPnpmfile, pnpmfiles: [pnpmfile] })

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

test('ignoring the default pnpmfile if tryLoadDefaultPnpmfile is not set', () => {
  const { hooks } = requireHooks(path.join(__dirname, '__fixtures__/default'), {})
  expect(hooks.readPackage?.length).toBe(0)
})

test('loading the default pnpmfile if tryLoadDefaultPnpmfile is set to true', () => {
  const { hooks } = requireHooks(path.join(__dirname, '__fixtures__/default'), { tryLoadDefaultPnpmfile: true })
  expect(hooks.readPackage?.length).toBe(1)
})

test('calculatePnpmfileChecksum is undefined when pnpmfile does not exist', async () => {
  const { hooks } = requireHooks(__dirname, {})
  expect(hooks.calculatePnpmfileChecksum).toBeUndefined()
})

test('calculatePnpmfileChecksum resolves to hash string for existing pnpmfile', async () => {
  const pnpmfile = path.join(__dirname, '__fixtures__/readPackageNoObject.js')
  const { hooks } = requireHooks(__dirname, { pnpmfiles: [pnpmfile] })
  expect(typeof await hooks.calculatePnpmfileChecksum?.()).toBe('string')
})

test('calculatePnpmfileChecksum is undefined if pnpmfile even when it exports undefined', async () => {
  const pnpmfile = path.join(__dirname, '__fixtures__/undefined.js')
  const { hooks } = requireHooks(__dirname, { pnpmfiles: [pnpmfile] })
  expect(hooks.calculatePnpmfileChecksum).toBeUndefined()
})

test('updateConfig throws an error if it returns undefined', async () => {
  const pnpmfile = path.join(__dirname, '__fixtures__/updateConfigReturnsUndefined.js')
  const { hooks } = requireHooks(__dirname, { pnpmfiles: [pnpmfile] })
  expect(() => hooks.updateConfig![0]!({})).toThrow('The updateConfig hook returned undefined')
})

test('requireHooks throw an error if one of the specified pnpmfiles does not exist', async () => {
  expect(() => requireHooks(__dirname, { pnpmfiles: ['does-not-exist.cjs'] })).toThrow('is not found')
})
