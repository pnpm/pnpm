import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  install,
  PackageManifest,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('readPackage, afterAllResolved hooks', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  function readPackageHook (pkg: PackageManifest) {
    switch (pkg.name) {
      case 'pkg-with-1-dep':
        if (!pkg.dependencies) {
          throw new Error('pkg-with-1-dep expected to have a dependencies field')
        }
        pkg.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
        break
    }
    return pkg
  }

  const afterAllResolved = sinon.spy((lockfile: Lockfile) => {
    lockfile['foo'] = 'foo' // tslint:disable-line
    return lockfile
  })

  await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({
    hooks: {
      afterAllResolved,
      readPackage: readPackageHook,
    },
  }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
  t.ok(afterAllResolved.calledOnce, 'afterAllResolved() called once')
  t.equal(afterAllResolved.getCall(0).args[0].lockfileVersion, 5)

  const wantedLockfile = await project.loadLockfile()
  t.equal(wantedLockfile['foo'], 'foo', 'the lockfile object has been updated by the hook') // tslint:disable-line:no-string-literal
})

// TODO: move this test to packages/pnpm
test['skip']('readPackage hook overrides project package', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  function readPackageHook (pkg: PackageManifest) {
    switch (pkg.name) {
      case 'test-read-package-hook':
        pkg.dependencies = { 'is-positive': '1.0.0' }
        break
    }
    return pkg
  }

  const pkg = await install({
    name: 'test-read-package-hook',
  }, await testDefaults({
    hooks: { readPackage: readPackageHook },
  }))

  await project.has('is-positive')

  t.notOk(pkg.dependencies, 'dependencies added by the hooks not saved in package.json')
})
