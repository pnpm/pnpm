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
  const glibcVariantIntegrity = 'sha256-glibc=='
  const muslVariantIntegrity = 'sha256-musl=='
  const darwinVariantIntegrity = 'sha256-darwin=='

  // Always-explicit selectors — don't rely on process.platform / host libc so
  // these tests produce the same result on glibc, musl, macOS, and Windows CI.
  const linuxGlibcSelector = { os: ['linux'], cpu: ['x64'], libc: ['glibc'] }
  const linuxMuslSelector = { os: ['linux'], cpu: ['x64'], libc: ['musl'] }
  const darwinSelector = { os: ['darwin'], cpu: ['arm64'] }

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
          // Linux default (glibc) — variant has no libc marker.
          targets: [{ os: 'linux', cpu: 'x64' }],
          resolution: variantResolution(glibcVariantIntegrity),
        },
        {
          targets: [{ os: 'linux', cpu: 'x64', libc: 'musl' as const }],
          resolution: variantResolution(muslVariantIntegrity),
        },
        {
          targets: [{ os: 'darwin', cpu: 'arm64' }],
          resolution: variantResolution(darwinVariantIntegrity),
        },
      ],
    },
  }

  function graphFor (selector: Parameters<typeof lockfileToDepGraph>[1]) {
    return lockfileToDepGraph(
      {
        lockfileVersion: '9.0',
        importers: {},
        packages: {
          ['node@runtime:22.0.0' as DepPath]: pkgWithVariants,
        },
      },
      selector
    )
  }

  test('picks the linux glibc variant when supportedArchitectures matches it', () => {
    expect(graphFor(linuxGlibcSelector)['node@runtime:22.0.0' as DepPath].fullPkgId)
      .toBe(`node@runtime:22.0.0:${glibcVariantIntegrity}`)
  })

  test('picks the linux musl variant when supportedArchitectures.libc=musl', () => {
    expect(graphFor(linuxMuslSelector)['node@runtime:22.0.0' as DepPath].fullPkgId)
      .toBe(`node@runtime:22.0.0:${muslVariantIntegrity}`)
  })

  test('picks the darwin variant when supportedArchitectures.os=darwin', () => {
    expect(graphFor(darwinSelector)['node@runtime:22.0.0' as DepPath].fullPkgId)
      .toBe(`node@runtime:22.0.0:${darwinVariantIntegrity}`)
  })

  test('different variants produce different fullPkgIds for the same runtime version', () => {
    const glibc = graphFor(linuxGlibcSelector)['node@runtime:22.0.0' as DepPath].fullPkgId
    const musl = graphFor(linuxMuslSelector)['node@runtime:22.0.0' as DepPath].fullPkgId
    const darwin = graphFor(darwinSelector)['node@runtime:22.0.0' as DepPath].fullPkgId
    expect(new Set([glibc, musl, darwin]).size).toBe(3)
  })
})
