import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import type { CustomFetcher, CustomResolver } from '@pnpm/hooks.types'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'

import { testDefaults } from '../utils/index.js'

test.each(['custom', 'tarball'])('hoisted reinstall refreshes a directory delegated from a %s resolution', async (resolutionType) => {
  prepareEmpty()
  const source = path.resolve('source')
  fs.mkdirSync(source)
  fs.writeFileSync(path.join(source, 'package.json'), JSON.stringify({ name: 'delegated-pkg', version: '1.0.0' }))
  fs.writeFileSync(path.join(source, 'index.js'), 'first')
  fs.symlinkSync('index.js', path.join(source, 'linked.js'), 'file')
  const customResolver: CustomResolver = {
    canResolve: (descriptor) => descriptor.alias === 'delegated-pkg',
    resolve: async () => ({
      id: 'delegated-pkg@1.0.0',
      resolution: resolutionType === 'custom'
        ? { type: 'custom:directory' }
        : { tarball: 'file:delegated-pkg.tgz' },
    }),
  }
  const customFetcher: CustomFetcher = {
    canFetch: (pkgId) => pkgId === 'delegated-pkg@1.0.0',
    fetch: () => ({ delegate: { type: 'directory', directory: source } }),
  }
  const options = testDefaults({
    fastUnpack: false,
    nodeLinker: 'hoisted',
    packageImportMethod: 'copy',
    customResolvers: [customResolver],
    customFetchers: [customFetcher],
  }, undefined, { localDirPackageImportMethod: 'clone-or-copy', resolveSymlinksInInjectedDirs: false })
  const { updatedManifest } = await addDependenciesToPackage({}, ['delegated-pkg@1.0.0'], options)
  const installed = path.resolve('node_modules/delegated-pkg')
  expect(fs.readFileSync(path.join(installed, 'index.js'), 'utf8')).toBe('first')
  expect(fs.lstatSync(path.join(installed, 'linked.js')).isSymbolicLink()).toBe(true)

  fs.writeFileSync(path.join(source, 'index.js'), 'second')
  fs.writeFileSync(path.join(source, 'new.js'), 'new')
  await install(updatedManifest, testDefaults({
    fastUnpack: false,
    nodeLinker: 'hoisted',
    packageImportMethod: 'copy',
    frozenLockfile: true,
    storeDir: options.storeDir,
    customFetchers: [customFetcher],
  }, undefined, { localDirPackageImportMethod: 'clone-or-copy', resolveSymlinksInInjectedDirs: false }))

  expect(fs.readFileSync(path.join(installed, 'index.js'), 'utf8')).toBe('second')
  expect(fs.readFileSync(path.join(installed, 'new.js'), 'utf8')).toBe('new')
  expect(fs.lstatSync(path.join(installed, 'linked.js')).isSymbolicLink()).toBe(true)
})

// Mirrors pacquet's `crates/cli/tests/custom_fetchers.rs`: a custom resolver
// writes a custom-typed resolution and the sibling fetcher materializes it by
// returning the portable `{ delegate }` envelope — the shape that works
// identically in both stacks.

test.each(['isolated', 'hoisted'] as const)('custom fetcher delegates a custom-typed resolution via the { delegate } envelope with %s linking', async (nodeLinker) => {
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

  const options = testDefaults({
    nodeLinker,
    customResolvers: [customResolver],
    customFetchers: [delegatingFetcher],
  })
  const { updatedManifest } = await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'],
    options
  )

  expect(fetchCalls).toBe(1)
  project.has('@pnpm.e2e/dep-of-pkg-with-1-dep')

  const reinstallOptions = testDefaults({
    nodeLinker,
    frozenLockfile: true,
    storeDir: options.storeDir,
    customFetchers: [delegatingFetcher],
  })
  const importPackage = jest.spyOn(reinstallOptions.storeController, 'importPackage')
  await install(updatedManifest, reinstallOptions)
  expect(importPackage).not.toHaveBeenCalled()
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
