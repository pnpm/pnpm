import { expect, jest, test } from '@jest/globals'
import type { PackageManifest, ReadPackageHook } from '@pnpm/types'

import { createReadPackageHook, getEffectivePackageExtensions } from '../lib/createReadPackageHook.js'

test('createReadPackageHook() is passing directory to all hooks', async () => {
  const hook1 = jest.fn(((manifest) => manifest) as ReadPackageHook)
  const hook2 = jest.fn(((manifest) => manifest) as ReadPackageHook)
  const readPackageHook = createReadPackageHook({
    ignoreCompatibilityDb: true,
    lockfileDir: '/foo',
    readPackageHook: [hook1, hook2],
  })
  const manifest = {}
  const dir = '/bar'
  await readPackageHook!(manifest, dir)
  expect(hook1).toHaveBeenCalledWith(manifest, dir)
  expect(hook2).toHaveBeenCalledWith(manifest, dir)
})

test('createReadPackageHook() runs the custom hook before the version overrider', async () => {
  const hook = jest.fn(((manifest) => ({
    ...manifest,
    dependencies: {
      ...manifest.dependencies,
      react: '18',
    },
  })) as ReadPackageHook)
  const readPackageHook = createReadPackageHook({
    ignoreCompatibilityDb: true,
    lockfileDir: '/foo',
    readPackageHook: [hook],
    overrides: [
      {
        targetPkg: {
          name: 'react',
        },
        newBareSpecifier: '16',
      },
    ],
  })
  const manifest = {}
  const dir = '/bar'
  const updatedManifest = await readPackageHook!(manifest, dir)
  expect(hook).toHaveBeenCalledWith(manifest, dir)
  expect(updatedManifest).toStrictEqual({
    dependencies: {
      react: '16',
    },
  })
})

test('createReadPackageHook() does not apply compatibility extensions to project manifests', async () => {
  const readPackageHook = createReadPackageHook({
    lockfileDir: '/project',
    packageExtensions: {
      'vue-loader': {
        dependencies: {
          custom: '1.0.0',
        },
      },
    },
  })

  const updatedManifest = await readPackageHook!({
    name: 'vue-loader',
    version: '0.0.0',
  }, '/project')

  expect(updatedManifest).toStrictEqual({
    dependencies: {
      custom: '1.0.0',
    },
    name: 'vue-loader',
    version: '0.0.0',
  })
})

test('createReadPackageHook() applies compatibility extensions to dependency manifests', async () => {
  const readPackageHook = createReadPackageHook({
    lockfileDir: '/project',
  })

  const manifest: PackageManifest = {
    name: 'vue-loader',
    version: '0.0.0',
  }
  const updatedManifest = await readPackageHook!(manifest)

  expect(updatedManifest.peerDependencies).toStrictEqual({
    '@vue/compiler-sfc': '^3.0.8',
    webpack: '^4.1.0 || ^5.0.0-0',
  })
})

test('getEffectivePackageExtensions() merges duplicate compatibility selectors', () => {
  expect(getEffectivePackageExtensions({})?.['gatsby-core-utils@<2.14.0-next.1']).toStrictEqual({
    dependencies: {
      '@babel/runtime': '^7.14.8',
      got: '8.3.2',
    },
  })
})
