import { expect, test } from '@jest/globals'
import type { CustomFetcher, CustomResolver } from '@pnpm/hooks.types'
import { addDependenciesToPackage } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'

import { testDefaults } from '../utils/index.js'

// Mirrors pacquet's `crates/cli/tests/custom_fetchers.rs`: a custom resolver
// writes a custom-typed resolution and the sibling fetcher materializes it by
// returning the portable `{ delegate }` envelope — the shape that works
// identically in both stacks.

test('custom fetcher delegates a custom-typed resolution via the { delegate } envelope', async () => {
  const project = prepareEmpty()

  const customResolver: CustomResolver = {
    canResolve: (descriptor) => descriptor.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep',
    resolve: async () => ({
      id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0',
      resolution: {
        type: 'custom:e2e',
        url: `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.0.0.tgz`,
        integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0'),
      },
    }),
  }

  let fetchCalls = 0
  const delegatingFetcher: CustomFetcher = {
    canFetch: (_pkgId, resolution) => resolution.type === 'custom:e2e',
    fetch: (_cafs, resolution) => {
      fetchCalls++
      return {
        delegate: {
          tarball: (resolution as { url: string }).url,
          integrity: (resolution as { integrity: string }).integrity,
        },
      }
    },
  }

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'],
    testDefaults({
      customResolvers: [customResolver],
      customFetchers: [delegatingFetcher],
    })
  )

  expect(fetchCalls).toBe(1)
  project.has('@pnpm.e2e/dep-of-pkg-with-1-dep')
})

test('a custom-typed resolution without a claiming fetcher fails the install', async () => {
  prepareEmpty()

  const customResolver: CustomResolver = {
    canResolve: (descriptor) => descriptor.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep',
    resolve: async () => ({
      id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0',
      resolution: {
        type: 'custom:e2e',
        url: `http://localhost:${REGISTRY_MOCK_PORT}/@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.0.0.tgz`,
        integrity: getIntegrity('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0'),
      },
    }),
  }

  await expect(
    addDependenciesToPackage(
      {},
      ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'],
      testDefaults({
        customResolvers: [customResolver],
      })
    )
  ).rejects.toThrow('Cannot fetch dependency with custom resolution type "custom:e2e"')
})
