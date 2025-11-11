import { getManifestFromResponse, type WantedDependency } from '../lib/resolveDependencies.js'
import type { PackageResponse } from '@pnpm/store-controller-types'

test('getManifestFromResponse returns manifest from pkgResponse when available', () => {
  const pkgResponse = {
    body: {
      manifest: {
        name: 'foo',
        version: '1.0.0',
      },
    },
  } as PackageResponse

  const wantedDependency = {
    alias: 'foo',
    bareSpecifier: 'foo',
    dev: false,
    optional: false,
  } as WantedDependency

  const result = getManifestFromResponse(pkgResponse, wantedDependency)

  expect(result).toEqual({
    name: 'foo',
    version: '1.0.0',
  })
})

test('getManifestFromResponse returns currentPkg info when manifest is undefined', () => {
  const pkgResponse = {
    body: {
      manifest: undefined,
    },
  } as PackageResponse

  const wantedDependency = {
    alias: 'node',
    bareSpecifier: 'runtime:^22.0.0',
    dev: false,
    optional: false,
  } as WantedDependency

  const currentPkg = {
    name: 'node',
    version: '22.20.0',
  }

  const result = getManifestFromResponse(pkgResponse, wantedDependency, currentPkg)

  expect(result).toEqual({
    name: 'node',
    version: '22.20.0',
  })
})

test('getManifestFromResponse returns default 0.0.0 when manifest and currentPkg are unavailable', () => {
  const pkgResponse = {
    body: {
      manifest: undefined,
    },
  } as PackageResponse

  const wantedDependency = {
    alias: 'foo',
    bareSpecifier: 'foo@^1.0.0',
    dev: false,
    optional: false,
  } as WantedDependency

  const result = getManifestFromResponse(pkgResponse, wantedDependency)

  expect(result).toEqual({
    name: 'foo',
    version: '0.0.0',
  })
})

test('getManifestFromResponse extracts name from bareSpecifier when no alias', () => {
  const pkgResponse = {
    body: {
      manifest: undefined,
    },
  } as PackageResponse

  const wantedDependency = {
    bareSpecifier: '@scope/package@^1.0.0',
    dev: false,
    optional: false,
  } as WantedDependency

  const result = getManifestFromResponse(pkgResponse, wantedDependency)

  expect(result).toEqual({
    name: 'package@^1.0.0',
    version: '0.0.0',
  })
})

test('getManifestFromResponse does not use currentPkg when only name is available', () => {
  const pkgResponse = {
    body: {
      manifest: undefined,
    },
  } as PackageResponse

  const wantedDependency = {
    alias: 'foo',
    bareSpecifier: 'foo',
    dev: false,
    optional: false,
  } as WantedDependency

  const currentPkg = {
    name: 'foo',
    version: undefined,
  }

  const result = getManifestFromResponse(pkgResponse, wantedDependency, currentPkg)

  expect(result).toEqual({
    name: 'foo',
    version: '0.0.0',
  })
})

test('getManifestFromResponse does not use currentPkg when only version is available', () => {
  const pkgResponse = {
    body: {
      manifest: undefined,
    },
  } as PackageResponse

  const wantedDependency = {
    alias: 'foo',
    bareSpecifier: 'foo',
    dev: false,
    optional: false,
  } as WantedDependency

  const currentPkg = {
    name: undefined,
    version: '1.0.0',
  }

  const result = getManifestFromResponse(pkgResponse, wantedDependency, currentPkg)

  expect(result).toEqual({
    name: 'foo',
    version: '0.0.0',
  })
})
