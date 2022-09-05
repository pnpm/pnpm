import path from 'path'
import { requireHooks, requirePnpmfile, BadReadPackageHookError } from '@pnpm/pnpmfile'

test('ignoring a pnpmfile that exports undefined', () => {
  const pnpmfile = requirePnpmfile(path.join(__dirname, 'pnpmfiles/undefined.js'), __dirname)
  expect(pnpmfile).toBeUndefined()
})

test('readPackage hook run fails when returns undefined ', () => {
  const pnpmfilePath = path.join(__dirname, 'pnpmfiles/readPackageNoReturn.js')
  const pnpmfile = requirePnpmfile(pnpmfilePath, __dirname)

  return expect(
    pnpmfile.hooks.readPackage({})
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook did not return a package manifest object.'))
})

test('readPackage hook run fails when returned dependencies is not an object ', () => {
  const pnpmfilePath = path.join(__dirname, 'pnpmfiles/readPackageNoObject.js')
  const pnpmfile = requirePnpmfile(pnpmfilePath, __dirname)
  return expect(
    pnpmfile.hooks.readPackage({})
  ).rejects.toEqual(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned package manifest object\'s property \'dependencies\' must be an object.'))
})

test('filterLog hook works from single source', () => {
  const pnpmfile = path.join(__dirname, 'pnpmfiles/filterLog.js')
  const hooks = requireHooks(__dirname, { pnpmfile })

  expect(hooks.filterLog).toBeDefined()
  expect(hooks.filterLog!({
    name: 'pnpm:summary',
    level: 'error',
    prefix: 'test',
  })).toBeTruthy()
  expect(hooks.filterLog!({
    name: 'pnpm:summary',
    level: 'warn',
    prefix: 'test',
    message: 'message',
  })).toBeTruthy()
  expect(hooks.filterLog!({
    name: 'pnpm:summary',
    level: 'debug',
    prefix: 'test',
  })).toBeTruthy()
})

test('filterLog hook combines with the global hook', () => {
  const globalPnpmfile = path.join(__dirname, 'pnpmfiles/globalFilterLog.js')
  const pnpmfile = path.join(__dirname, 'pnpmfiles/filterLog.js')
  const hooks = requireHooks(__dirname, { globalPnpmfile, pnpmfile })

  expect(hooks.filterLog).toBeDefined()
  expect(hooks.filterLog!({
    name: 'pnpm:summary',
    level: 'error',
    prefix: 'test',
  })).toBeTruthy()
  expect(hooks.filterLog!({
    name: 'pnpm:summary',
    level: 'warn',
    prefix: 'test',
    message: 'message',
  })).toBeTruthy()
  expect(hooks.filterLog!({
    name: 'pnpm:summary',
    level: 'debug',
    prefix: 'test',
  })).toBeFalsy()
})

test('filterLog hook combines with the global hook and the options hook', () => {
  const globalPnpmfile = path.join(__dirname, 'pnpmfiles/globalFilterLog.js')
  const pnpmfile = path.join(__dirname, 'pnpmfiles/filterLog.js')
  const hooks = requireHooks(__dirname, {
    globalPnpmfile,
    pnpmfile,
    hooks: {
      filterLog (log) {
        return log.level === 'error'
      },
    },
  })

  expect(hooks.filterLog).toBeDefined()
  expect(hooks.filterLog!({
    name: 'pnpm:summary',
    level: 'error',
    prefix: 'test',
  })).toBeTruthy()
  expect(hooks.filterLog!({
    name: 'pnpm:summary',
    level: 'warn',
    prefix: 'test',
    message: 'message',
  })).toBeFalsy()
  expect(hooks.filterLog!({
    name: 'pnpm:summary',
    level: 'debug',
    prefix: 'test',
  })).toBeFalsy()
})

test('readPackage hook works from single source', async () => {
  const pnpmfile = path.join(__dirname, 'pnpmfiles/readPackage.js')
  const hooks = requireHooks(__dirname, { pnpmfile })

  expect(hooks.readPackage).toBeDefined()
  console.log(hooks.readPackage!({}))
  expect(await hooks.readPackage!({})).toHaveProperty('local', true)
  expect(await hooks.readPackage!({})).not.toHaveProperty('global', true)
  expect(await hooks.readPackage!({})).not.toHaveProperty('opts', true)
})

test('readPackage hook combines with the global hook', async () => {
  const pnpmfile = path.join(__dirname, 'pnpmfiles/readPackage.js')
  const globalPnpmfile = path.join(__dirname, 'pnpmfiles/globalReadPackage.js')
  const hooks = requireHooks(__dirname, { pnpmfile, globalPnpmfile })

  expect(hooks.readPackage).toBeDefined()
  expect(await hooks.readPackage!({})).toHaveProperty('local', true)
  expect(await hooks.readPackage!({})).toHaveProperty('global', true)
  expect(await hooks.readPackage!({})).not.toHaveProperty('opts', true)
})

test('readPackage hook combines with the global hook and the options hook', async () => {
  const pnpmfile = path.join(__dirname, 'pnpmfiles/readPackage.js')
  const globalPnpmfile = path.join(__dirname, 'pnpmfiles/globalReadPackage.js')
  const hooks = requireHooks(__dirname, {
    pnpmfile,
    globalPnpmfile,
    hooks: {
      readPackage (pkg) {
        pkg.opts = true
        return pkg
      },
    },
  })

  expect(hooks.readPackage).toBeDefined()
  expect(await hooks.readPackage!({})).toHaveProperty('local', true)
  expect(await hooks.readPackage!({})).toHaveProperty('global', true)
  expect(await hooks.readPackage!({})).toHaveProperty('opts', true)
})