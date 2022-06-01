import PnpmError from '@pnpm/error'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, mutateModules } from '@pnpm/core'
import { createObjectChecksum } from '../../lib/install/index'
import {
  testDefaults,
} from '../utils'

test('manifests are extended with fields specified by packageExtensions', async () => {
  const project = prepareEmpty()

  const packageExtensions = {
    'is-positive': {
      dependencies: {
        bar: '100.1.0',
      },
    },
  }
  const manifest = await addDependenciesToPackage(
    {},
    ['is-positive@1.0.0'],
    await testDefaults({ packageExtensions })
  )

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/is-positive/1.0.0'].dependencies?.['bar']).toBe('100.1.0')
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(createObjectChecksum({
      'is-positive': {
        dependencies: {
          bar: '100.1.0',
        },
      },
    }))
    const currentLockfile = await project.readCurrentLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(currentLockfile.packageExtensionsChecksum)
  }

  // The lockfile is updated if the overrides are changed
  packageExtensions['is-positive'].dependencies!['foobar'] = '100.0.0'
  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ packageExtensions }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/is-positive/1.0.0'].dependencies?.['foobar']).toBe('100.0.0')
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(createObjectChecksum({
      'is-positive': {
        dependencies: {
          bar: '100.1.0',
          foobar: '100.0.0',
        },
      },
    }))
    const currentLockfile = await project.readCurrentLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(currentLockfile.packageExtensionsChecksum)
  }

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ frozenLockfile: true, packageExtensions }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(createObjectChecksum({
      'is-positive': {
        dependencies: {
          bar: '100.1.0',
          foobar: '100.0.0',
        },
      },
    }))
    const currentLockfile = await project.readCurrentLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(currentLockfile.packageExtensionsChecksum)
  }

  packageExtensions['is-positive'].dependencies!['bar'] = '100.0.1'
  await expect(
    mutateModules([
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ], await testDefaults({ frozenLockfile: true, packageExtensions }))
  ).rejects.toThrow(
    new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE',
      'Cannot perform a frozen installation because the lockfile needs updates'
    )
  )
})

test('manifests are patched by extensions from the compatibility database', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['debug@4.0.0'],
    await testDefaults()
  )

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/debug/4.0.0'].peerDependenciesMeta?.['supports-color']?.optional).toBe(true)
})
