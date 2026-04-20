import { lockfileToDepGraph } from '@pnpm/deps.graph-hasher'
import type { BinaryResolution } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'

test('lockfileToDepGraph', () => {
  expect(lockfileToDepGraph({
    lockfileVersion: '9.0',
    importers: {},
    packages: {
      ['foo@1.0.0' as DepPath]: {
        dependencies: {
          bar: '1.0.0',
        },
        optionalDependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: '0',
        },
      },
      ['bar@1.0.0' as DepPath]: {
        dependencies: {
          qar: '1.0.0',
        },
        resolution: {
          integrity: '1',
        },
      },
      ['qar@1.0.0' as DepPath]: {
        resolution: {
          integrity: '2',
        },
      },
    },
  })).toStrictEqual({
    'bar@1.0.0': {
      children: {
        qar: 'qar@1.0.0',
      },
      fullPkgId: 'bar@1.0.0:1',
    },
    'foo@1.0.0': {
      children: {
        bar: 'bar@1.0.0',
        qar: 'qar@1.0.0',
      },
      fullPkgId: 'foo@1.0.0:0',
    },
    'qar@1.0.0': {
      children: {},
      fullPkgId: 'qar@1.0.0:2',
    },
  })
})

describe('lockfileToDepGraph with variations resolution', () => {
  const hostVariantIntegrity = 'sha256-host=='
  const muslVariantIntegrity = 'sha256-musl=='
  const hostPlatform = process.platform
  const hostArch = process.arch

  function variantResolution (integrity: string): BinaryResolution {
    return {
      type: 'binary',
      archive: 'tarball',
      bin: 'bin/node',
      integrity,
      url: `https://example.com/${integrity}.tar.gz`,
    }
  }

  const pkgWithVariants = {
    resolution: {
      type: 'variations' as const,
      variants: [
        {
          targets: [{ os: hostPlatform, cpu: hostArch }],
          resolution: variantResolution(hostVariantIntegrity),
        },
        {
          targets: [{ os: 'linux', cpu: 'x64', libc: 'musl' as const }],
          resolution: variantResolution(muslVariantIntegrity),
        },
      ],
    },
  }

  test('picks the host variant by default and uses its integrity in fullPkgId', () => {
    const graph = lockfileToDepGraph({
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        ['node@runtime:22.0.0' as DepPath]: pkgWithVariants,
      },
    })
    expect(graph['node@runtime:22.0.0' as DepPath].fullPkgId)
      .toBe(`node@runtime:22.0.0:${hostVariantIntegrity}`)
  })

  test('incorporates the explicitly selected musl variant when supportedArchitectures.libc=musl', () => {
    const graph = lockfileToDepGraph(
      {
        lockfileVersion: '9.0',
        importers: {},
        packages: {
          ['node@runtime:22.0.0' as DepPath]: pkgWithVariants,
        },
      },
      { os: ['linux'], cpu: ['x64'], libc: ['musl'] }
    )
    expect(graph['node@runtime:22.0.0' as DepPath].fullPkgId)
      .toBe(`node@runtime:22.0.0:${muslVariantIntegrity}`)
  })

  test('different variants produce different fullPkgIds for the same runtime version', () => {
    const host = lockfileToDepGraph({
      lockfileVersion: '9.0',
      importers: {},
      packages: { ['node@runtime:22.0.0' as DepPath]: pkgWithVariants },
    })
    const musl = lockfileToDepGraph(
      {
        lockfileVersion: '9.0',
        importers: {},
        packages: { ['node@runtime:22.0.0' as DepPath]: pkgWithVariants },
      },
      { os: ['linux'], cpu: ['x64'], libc: ['musl'] }
    )
    expect(host['node@runtime:22.0.0' as DepPath].fullPkgId)
      .not.toBe(musl['node@runtime:22.0.0' as DepPath].fullPkgId)
  })
})
