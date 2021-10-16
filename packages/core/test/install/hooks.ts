import { LOCKFILE_VERSION } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import {
  addDependenciesToPackage,
  PackageManifest,
} from '@pnpm/core'
import sinon from 'sinon'
import {
  addDistTag,
  testDefaults,
} from '../utils'

test('readPackage, afterAllResolved hooks', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  function readPackageHook (manifest: PackageManifest) {
    switch (manifest.name) {
    case 'pkg-with-1-dep':
      if (manifest.dependencies == null) {
        throw new Error('pkg-with-1-dep expected to have a dependencies field')
      }
      manifest.dependencies['dep-of-pkg-with-1-dep'] = '100.0.0'
      break
    }
    return manifest
  }

  const afterAllResolved = sinon.spy((lockfile: Lockfile) => {
    lockfile['foo'] = 'foo' // eslint-disable-line
    return lockfile
  })

  await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({
    hooks: {
      afterAllResolved,
      readPackage: readPackageHook,
    },
  }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
  expect(afterAllResolved.calledOnce).toBeTruthy()
  expect(afterAllResolved.getCall(0).args[0].lockfileVersion).toEqual(LOCKFILE_VERSION)

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile['foo']).toEqual('foo') // eslint-disable-line @typescript-eslint/dot-notation
})
