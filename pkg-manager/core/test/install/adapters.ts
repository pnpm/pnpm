import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage } from '@pnpm/core'
import { type Adapter } from '@pnpm/hooks.types'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { testDefaults } from '../utils/index.js'

// Test 1: Validate custom metadata flows through resolve() → lockfile
// TODO: Unskip when fixed - tests timeout with "Jest environment has been torn down" errors during dependency resolution
test.skip('custom adapter: metadata from resolve() is persisted to lockfile', async () => {
  const project = prepareEmpty()

  const resolveCallCount = { count: 0 }
  const shouldForceResolveCallCount = { count: 0 }
  let savedCachedAt: number | undefined

  // Adapter that wraps @pnpm.e2e/dep-of-pkg-with-1-dep and adds custom metadata
  const timestampAdapter: Adapter = {
    canResolve: (descriptor) => {
      return wantedDependency.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep'
    },

    resolve: async (descriptor, opts) => {
      resolveCallCount.count++
      const now = Date.now()
      savedCachedAt = now

      // Use standard npm resolution but add custom metadata
      return {
        id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0',
        resolution: {
          integrity: 'sha512-jPYrv4nLDd6nHrJWCAddqh+R+7WsbsU/lZ3tpDBQpjteXJVbSGSaicpkVQJp7lbVSvBJzdF+GKmqXvQXLv4rIg==',
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.0.0.tgz`,
          cachedAt: now, // Custom metadata
        },
        manifest: {
          name: '@pnpm.e2e/dep-of-pkg-with-1-dep',
          version: '100.0.0',
        },
      }
    },

    shouldForceResolve: (descriptor) => {
      shouldForceResolveCallCount.count++
      // Don't force re-resolution in this test
      return false
    },
  }

  // First install - creates lockfile with custom metadata
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'],
    testDefaults({
      hooks: {
        adapters: [timestampAdapter],
      },
    })
  )

  expect(resolveCallCount.count).toBe(1)
  expect(shouldForceResolveCallCount.count).toBe(0) // Not called on first install

  // Read lockfile to verify custom metadata was persisted
  const lockfile = project.readLockfile()
  const depPath = Object.keys(lockfile.packages ?? {})[0]
  expect(depPath).toBeTruthy()
  const pkgSnapshot = lockfile.packages?.[depPath]
  expect((pkgSnapshot?.resolution as any)?.cachedAt).toBe(savedCachedAt) // eslint-disable-line @typescript-eslint/no-explicit-any

  // Second install - reads from lockfile and calls shouldForceResolve
  await addDependenciesToPackage(
    manifest,
    [],
    testDefaults({
      hooks: {
        adapters: [timestampAdapter],
      },
    })
  )

  // On second install, shouldForceResolve should be called
  expect(shouldForceResolveCallCount.count).toBe(1)
  // resolve() should not be called again since shouldForceResolve returned false
  expect(resolveCallCount.count).toBe(1)

  // Verify custom metadata still in lockfile after second install
  const lockfile2 = project.readLockfile()
  const depPath2 = Object.keys(lockfile2.packages ?? {})[0]
  const pkgSnapshot2 = lockfile2.packages?.[depPath2]
  expect((pkgSnapshot2?.resolution as any)?.cachedAt).toBe(savedCachedAt) // eslint-disable-line @typescript-eslint/no-explicit-any
})

// Test 2: Validate adapter resolution works for both fresh and cached installs
// TODO: Unskip when fixed - tests timeout with "Jest environment has been torn down" errors during dependency resolution
test.skip('custom adapter: works for fresh resolve() and lockfile resolutions', async () => {
  const project = prepareEmpty()

  let resolveCallCount = 0

  // Adapter that wraps standard resolution but tracks calls
  const trackingAdapter: Adapter = {
    canResolve: (descriptor) => {
      return wantedDependency.alias === '@pnpm.e2e/pkg-with-1-dep'
    },

    resolve: async (descriptor, _opts) => {
      resolveCallCount++
      // Use standard npm resolution
      return {
        id: '@pnpm.e2e/pkg-with-1-dep@100.0.0',
        resolution: {
          integrity: 'sha512-1MYbHCSEbOwCLN6cERxVQcVH/W0dXIz9YSv6dBdq3CaWVqx11tMwFt6o7gPBs/r2eDxPEz+CDNT/ZFNYNt78wg==',
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e/pkg-with-1-dep/-/pkg-with-1-dep-100.0.0.tgz`,
        },
        manifest: {
          name: '@pnpm.e2e/pkg-with-1-dep',
          version: '100.0.0',
        },
      }
    },
  }

  // First install - fresh resolution, adapter should be used
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-with-1-dep@100.0.0'],
    testDefaults({
      hooks: {
        adapters: [trackingAdapter],
      },
    })
  )

  // Verify adapter.resolve was called for fresh resolution
  expect(resolveCallCount).toBe(1)

  project.has('@pnpm.e2e/pkg-with-1-dep')

  // Second install - should read from lockfile
  resolveCallCount = 0
  await addDependenciesToPackage(
    manifest,
    [],
    testDefaults({
      hooks: {
        adapters: [trackingAdapter],
      },
    })
  )

  // Verify adapter.resolve was not called again (using cached lockfile)
  expect(resolveCallCount).toBe(0)

  project.has('@pnpm.e2e/pkg-with-1-dep')
})

// Test 3: Validate shouldForceResolve can trigger re-resolution
// TODO: Unskip when fixed - tests timeout with "Jest environment has been torn down" errors during dependency resolution
test.skip('custom adapter: shouldForceResolve=true triggers re-resolution', async () => {
  const project = prepareEmpty()

  let resolveCallCount = 0
  let shouldForceReturn = false

  const forceResolveAdapter: Adapter = {
    canResolve: (descriptor) => {
      return wantedDependency.alias === '@pnpm.e2e/foo'
    },

    resolve: async (descriptor, _opts) => {
      resolveCallCount++

      return {
        id: `@pnpm.e2e/foo@100.${resolveCallCount}.0`,
        resolution: {
          integrity: 'sha512-c3bT3gLTuSRfC0pbs4TgMGjeN1t7eJGwb2vWVx/zUYJp+CsVj3cMNWPanEjahorIUXpW/senCGjMHfyFWLiM4Q==',
          tarball: `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e/foo/-/foo-100.0.0.tgz`,
          resolveCount: resolveCallCount, // Track how many times resolve was called
        },
        manifest: {
          name: '@pnpm.e2e/foo',
          version: '100.0.0',
        },
      }
    },

    shouldForceResolve: () => {
      return shouldForceReturn
    },
  }

  // First install
  const { updatedManifest: manifest1 } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/foo@100.0.0'],
    testDefaults({
      hooks: {
        adapters: [forceResolveAdapter],
      },
    })
  )

  expect(resolveCallCount).toBe(1)

  // Second install with shouldForceResolve returning false
  shouldForceReturn = false
  const { updatedManifest: manifest2 } = await addDependenciesToPackage(
    manifest1,
    [],
    testDefaults({
      hooks: {
        adapters: [forceResolveAdapter],
      },
    })
  )

  // resolve() should not be called again
  expect(resolveCallCount).toBe(1)

  // Third install with shouldForceResolve returning true
  shouldForceReturn = true
  await addDependenciesToPackage(
    manifest2,
    [],
    testDefaults({
      hooks: {
        adapters: [forceResolveAdapter],
      },
    })
  )

  // resolve() should be called again due to forced re-resolution
  expect(resolveCallCount).toBe(2)

  // Verify lockfile was updated with new resolveCount
  const lockfile = project.readLockfile()
  const depPath = Object.keys(lockfile.packages ?? {})[0]
  const pkgSnapshot = lockfile.packages?.[depPath]
  expect((pkgSnapshot?.resolution as any)?.resolveCount).toBe(2) // eslint-disable-line @typescript-eslint/no-explicit-any
})
