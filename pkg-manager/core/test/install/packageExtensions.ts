import { PnpmError } from '@pnpm/error'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, mutateModulesInSingleProject, install } from '@pnpm/core'
import { type ProjectRootDir, type PackageExtension, type ProjectManifest } from '@pnpm/types'
import { createObjectChecksum } from '../../lib/install/index'
import {
  testDefaults,
} from '../utils'

test('manifests are extended with fields specified by packageExtensions', async () => {
  const project = prepareEmpty()

  const packageExtensions: Record<string, PackageExtension> = {
    'is-positive': {
      dependencies: {
        '@pnpm.e2e/bar': '100.1.0',
      },
    },
  }
  const manifest = await addDependenciesToPackage(
    {},
    ['is-positive@1.0.0'],
    testDefaults({ packageExtensions })
  )

  {
    const lockfile = project.readLockfile()
    expect(lockfile.snapshots['is-positive@1.0.0'].dependencies?.['@pnpm.e2e/bar']).toBe('100.1.0')
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(createObjectChecksum({
      'is-positive': {
        dependencies: {
          '@pnpm.e2e/bar': '100.1.0',
        },
      },
    }))
    const currentLockfile = project.readCurrentLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(currentLockfile.packageExtensionsChecksum)
  }

  // The lockfile is updated if the overrides are changed
  packageExtensions['is-positive'].dependencies!['@pnpm.e2e/foobar'] = '100.0.0'
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ packageExtensions }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.snapshots['is-positive@1.0.0'].dependencies?.['@pnpm.e2e/foobar']).toBe('100.0.0')
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(createObjectChecksum({
      'is-positive': {
        dependencies: {
          '@pnpm.e2e/bar': '100.1.0',
          '@pnpm.e2e/foobar': '100.0.0',
        },
      },
    }))
    const currentLockfile = project.readCurrentLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(currentLockfile.packageExtensionsChecksum)
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true, packageExtensions }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(createObjectChecksum({
      'is-positive': {
        dependencies: {
          '@pnpm.e2e/bar': '100.1.0',
          '@pnpm.e2e/foobar': '100.0.0',
        },
      },
    }))
    const currentLockfile = project.readCurrentLockfile()
    expect(lockfile.packageExtensionsChecksum).toStrictEqual(currentLockfile.packageExtensionsChecksum)
  }

  packageExtensions['is-positive'].dependencies!['@pnpm.e2e/bar'] = '100.0.1'
  await expect(
    mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults({ frozenLockfile: true, packageExtensions }))
  ).rejects.toThrow(
    new PnpmError('LOCKFILE_CONFIG_MISMATCH',
      'Cannot proceed with the frozen installation. The current "packageExtensionsChecksum" configuration doesn\'t match the value found in the lockfile'
    )
  )
})

test('packageExtensionsChecksum does not change regardless of keys order', async () => {
  const project = prepareEmpty()

  const packageExtensions1: Record<string, PackageExtension> = {
    'is-odd': {
      peerDependencies: {
        'is-number': '*',
      },
    },
    'is-even': {
      peerDependencies: {
        'is-number': '*',
      },
    },
  }

  const packageExtensions2: Record<string, PackageExtension> = {
    'is-even': {
      peerDependencies: {
        'is-number': '*',
      },
    },
    'is-odd': {
      peerDependencies: {
        'is-number': '*',
      },
    },
  }

  const manifest = (): ProjectManifest => ({
    dependencies: {
      'is-even': '*',
      'is-odd': '*',
    },
  })

  await install(manifest(), testDefaults({
    packageExtensions: packageExtensions1,
  }))
  const lockfile1 = project.readLockfile()
  const checksum1 = lockfile1.packageExtensionsChecksum

  await install(manifest(), testDefaults({
    packageExtensions: packageExtensions2,
  }))
  const lockfile2 = project.readLockfile()
  const checksum2 = lockfile2.packageExtensionsChecksum

  expect(checksum1).toBe(checksum2)
  expect(checksum1).not.toBeFalsy()
})

test('manifests are patched by extensions from the compatibility database', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['debug@4.0.0'],
    testDefaults()
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages['debug@4.0.0'].peerDependenciesMeta?.['supports-color']?.optional).toBe(true)
})

test('manifests are not patched by extensions from the compatibility database when ignoreCompatibilityDb is true', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['debug@4.0.0'],
    testDefaults({
      ignoreCompatibilityDb: true,
    })
  )

  const lockfile = project.readLockfile()
  expect(lockfile.packages['debug@4.0.0'].peerDependenciesMeta).toBeUndefined()
})
