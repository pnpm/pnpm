import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { type CustomResolver } from '@pnpm/hooks.types'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { testDefaults } from '../utils/index.js'

// Integration tests for custom resolvers
// These tests verify that custom resolvers work correctly in the full install flow

// Test: Custom resolver is called during install
test('custom resolver is called during install', async () => {
  prepareEmpty()

  let resolveCallCount = 0

  const trackingResolver: CustomResolver = {
    canResolve: (descriptor) => {
      return descriptor.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep'
    },

    resolve: async () => {
      resolveCallCount++
      // Use the correct integrity for the actual package
      return {
        id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0',
        resolution: {
          integrity: 'sha512-atUXGBNAbym4OioYcKt1qTjiH23CSfZ1K2N8JgCUewSE5gY/i9YoK7Ez6+CuEZbH+O3R+HKNrRIaZfnkv/93tg==',
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.0.0.tgz`,
        },
        manifest: {
          name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
          version: '100.0.0',
        },
      }
    },
  }

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'],
    testDefaults({
      customResolvers: [trackingResolver],
    })
  )

  expect(resolveCallCount).toBe(1)
})

// Test: Custom resolver receives currentPkg on subsequent installs
test('custom resolver receives currentPkg on subsequent installs', async () => {
  prepareEmpty()

  let resolveCallCount = 0
  let receivedCurrentPkg: unknown = null

  const trackingResolver: CustomResolver = {
    canResolve: (descriptor) => {
      return descriptor.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep'
    },

    resolve: async (_descriptor, opts) => {
      resolveCallCount++
      receivedCurrentPkg = opts.currentPkg

      // If we have currentPkg, return the existing resolution
      if (opts.currentPkg) {
        return {
          id: opts.currentPkg.id,
          resolution: opts.currentPkg.resolution,
        }
      }

      return {
        id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0',
        resolution: {
          integrity: 'sha512-atUXGBNAbym4OioYcKt1qTjiH23CSfZ1K2N8JgCUewSE5gY/i9YoK7Ez6+CuEZbH+O3R+HKNrRIaZfnkv/93tg==',
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.0.0.tgz`,
        },
        manifest: {
          name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
          version: '100.0.0',
        },
      }
    },
  }

  // First install
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'],
    testDefaults({
      customResolvers: [trackingResolver],
    })
  )

  expect(resolveCallCount).toBe(1)
  expect(receivedCurrentPkg).toBeUndefined()

  // Second install - should receive currentPkg
  receivedCurrentPkg = null
  await addDependenciesToPackage(
    manifest,
    [],
    testDefaults({
      customResolvers: [trackingResolver],
    })
  )

  expect(resolveCallCount).toBe(2)
  expect(receivedCurrentPkg).toBeTruthy()
  expect((receivedCurrentPkg as any).id).toBe('@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0') // eslint-disable-line @typescript-eslint/no-explicit-any
})
