import { expect, jest, test } from '@jest/globals'
import type { ReadPackageHook } from '@pnpm/types'

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

test('getEffectivePackageExtensions() includes the pnpm-specific compatibility entries', () => {
  const extensions = getEffectivePackageExtensions({})
  expect(extensions?.['@angular/build@*']).toStrictEqual({
    dependencies: {
      tslib: '^2.3.0',
    },
  })
  expect(extensions?.['@nuxt/vite-builder@>=4.0.0 <4.5.0']).toStrictEqual({
    dependencies: {
      unplugin: '^2.3.5',
    },
  })
  expect(extensions?.['@nuxt/vite-builder@>=4.5.0']).toStrictEqual({
    dependencies: {
      unplugin: '^3.3.0',
    },
  })
  expect(getEffectivePackageExtensions({ ignoreCompatibilityDb: true })).toBeUndefined()
})

test('getEffectivePackageExtensions() merges duplicate compatibility selectors', () => {
  expect(getEffectivePackageExtensions({})?.['gatsby-core-utils@<2.14.0-next.1']).toStrictEqual({
    dependencies: {
      '@babel/runtime': '^7.14.8',
      got: '8.3.2',
    },
  })
})
