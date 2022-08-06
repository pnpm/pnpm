import { LOCKFILE_VERSION } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
  PackageManifest,
} from '@pnpm/core'
import { testDefaults } from '../utils'

test('readPackage, afterAllResolved hooks', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

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

  const afterAllResolved = jest.fn((lockfile: Lockfile) => {
    lockfile['foo'] = 'foo'
    return lockfile
  })

  await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({
    hooks: {
      afterAllResolved,
      readPackage: readPackageHook,
    },
  }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
  expect(afterAllResolved).toHaveBeenCalledTimes(1)
  expect(afterAllResolved.mock.calls[0][0].lockfileVersion).toEqual(LOCKFILE_VERSION)

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile['foo']).toEqual('foo')
})

test('readPackage, afterAllResolved async hooks', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: 'dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  async function readPackageHook (manifest: PackageManifest) {
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

  const afterAllResolved = jest.fn(async (lockfile: Lockfile) => {
    lockfile['foo'] = 'foo'
    return lockfile
  })

  await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults({
    hooks: {
      afterAllResolved,
      readPackage: readPackageHook,
    },
  }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
  expect(afterAllResolved).toHaveBeenCalledTimes(1)
  expect(afterAllResolved.mock.calls[0][0].lockfileVersion).toEqual(LOCKFILE_VERSION)

  const wantedLockfile = await project.readLockfile()
  expect(wantedLockfile['foo']).toEqual('foo')
})
