import { jest } from '@jest/globals'
import { type ReadPackageHook } from '@pnpm/types'
import { createReadPackageHook } from '../lib/createReadPackageHook.js'

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
