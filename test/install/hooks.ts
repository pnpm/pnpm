import loadJsonFile = require('load-json-file')
import {
  install,
  installPkgs,
  PackageManifest,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)

test('readPackage hook', async (t: tape.Test) => {
  const project = prepare(t)

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  function readPackageHook (pkg: PackageManifest) {
    switch (pkg.name) {
      case 'pkg-with-1-dep':
        pkg!.dependencies!['dep-of-pkg-with-1-dep'] = '100.0.0'
        break
    }
    return pkg
  }

  await installPkgs(['pkg-with-1-dep'], await testDefaults({
    hooks: {readPackage: readPackageHook},
  }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('readPackage hook overrides project package', async (t: tape.Test) => {
  const project = prepare(t, {
    name: 'test-read-package-hook',
  })

  function readPackageHook (pkg: PackageManifest) {
    switch (pkg.name) {
      case 'test-read-package-hook':
        pkg.dependencies = {'is-positive': '1.0.0'}
        break
    }
    return pkg
  }

  await install(await testDefaults({
    hooks: {readPackage: readPackageHook},
  }))

  await project.has('is-positive')

  const packageJson = await loadJsonFile('package.json')
  t.notOk(packageJson.dependencies, 'dependencies added by the hooks not saved in package.json')
})
