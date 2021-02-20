import { requirePnpmfile, BadReadPackageHookError } from '@pnpm/pnpmfile'
import path = require('path')

test('ignoring a pnpmfile that exports undefined', async () => {
  const pnpmfile = await requirePnpmfile(path.join(__dirname, 'pnpmfiles/undefined.js'), __dirname)
  expect(pnpmfile).toBeUndefined()
})

test('readPackage hook run fails when returns undefined ', async () => {
  const pnpmfilePath = path.join(__dirname, 'pnpmfiles/readPackageNoReturn.js')
  const pnpmfile = await requirePnpmfile(pnpmfilePath, __dirname)

  expect(() => {
    pnpmfile.hooks.readPackage({})
  }).toThrow(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook did not return a package manifest object.'))
})

test('readPackage hook run fails when returned dependencies is not an object ', async () => {
  const pnpmfilePath = path.join(__dirname, 'pnpmfiles/readPackageNoObject.js')
  const pnpmfile = await requirePnpmfile(pnpmfilePath, __dirname)
  expect(() => {
    pnpmfile.hooks.readPackage({})
  }).toThrow(new BadReadPackageHookError(pnpmfilePath, 'readPackage hook returned package manifest object\'s property \'dependencies\' must be an object.'))
})

test('a pnpmfile written in ESM is read successfully', async () => {
  const pnpmfilePath = path.join(__dirname, 'pnpmfiles/pnpmfile.mjs')
  const pnpmfile = await requirePnpmfile(pnpmfilePath, __dirname)
  expect(pnpmfile.hooks).toBeTruthy()
})
