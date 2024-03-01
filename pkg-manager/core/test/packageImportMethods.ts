import fs from 'fs'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { sync as loadJsonFile } from 'load-json-file'
import { testDefaults } from './utils'

test('packageImportMethod can be set to copy', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['is-negative'], testDefaults({ fastUnpack: false }, {}, {}, { packageImportMethod: 'copy' }))

  const m = project.requireModule('is-negative')
  expect(m).toBeTruthy() // is-negative is available with packageImportMethod = copy
})

test('copy does not fail on package that self-requires itself', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/requires-itself'], testDefaults({}, {}, {}, { packageImportMethod: 'copy' }))

  const m = project.requireModule('@pnpm.e2e/requires-itself/package.json')
  expect(m).toBeTruthy() // requires-itself is available with packageImportMethod = copy

  const lockfile = project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/requires-itself@1.0.0'].dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
})

test('packages are updated in node_modules, when packageImportMethod is set to copy and modules manifest and current lockfile are incorrect', async () => {
  prepareEmpty()
  const opts = testDefaults({ fastUnpack: false, force: false, nodeLinker: 'hoisted' }, {}, {}, { packageImportMethod: 'copy' })

  await addDependenciesToPackage({}, ['is-negative@1.0.0'], opts)
  const modulesManifestContent = fs.readFileSync('node_modules/.modules.yaml')
  const currentLockfile = fs.readFileSync('node_modules/.pnpm/lock.yaml')
  {
    const pkg = loadJsonFile<any>('node_modules/is-negative/package.json') // eslint-disable-line
    expect(pkg.version).toBe('1.0.0')
  }
  await addDependenciesToPackage({}, ['is-negative@2.0.0'], opts)
  {
    const pkg = loadJsonFile<any>('node_modules/is-negative/package.json') // eslint-disable-line
    expect(pkg.version).toBe('2.0.0')
  }
  fs.writeFileSync('node_modules/.modules.yaml', modulesManifestContent, 'utf8')
  fs.writeFileSync('node_modules/.pnpm/lock.yaml', currentLockfile, 'utf8')
  await addDependenciesToPackage({}, ['is-negative@1.0.0'], opts)

  const pkg = loadJsonFile<any>('node_modules/is-negative/package.json') // eslint-disable-line
  expect(pkg.version).toBe('1.0.0')
})
