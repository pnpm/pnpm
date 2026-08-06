import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { calcGraphNodeHash, lockfileToDepGraph, type PkgMeta } from '@pnpm/deps.graph-hasher'
import type { BinaryResolution } from '@pnpm/resolving.resolver-base'
import type { DepPath } from '@pnpm/types'

interface LinkHashFixture {
  package: {
    key: DepPath
    name: string
    version: string
    alias: string
    integrity: string
  }
  posix: LinkHashCase[]
  win32: LinkHashCase[]
}

interface LinkHashCase {
  name: string
  lockfileDir: string
  target: string
  expectedLinkNode: DepPath
  expectedSlot: string
}

const linkHashFixture = JSON.parse(fs.readFileSync(
  path.resolve(import.meta.dirname, '../../../../fixtures/gvs-link-hash-parity.json'),
  'utf8'
)) as LinkHashFixture

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
      resolution: { integrity: '1' },
      fullPkgId: 'bar@1.0.0:1',
    },
    'foo@1.0.0': {
      children: {
        bar: 'bar@1.0.0',
        qar: 'qar@1.0.0',
      },
      resolution: { integrity: '0' },
      fullPkgId: 'foo@1.0.0:0',
    },
    'qar@1.0.0': {
      children: {},
      resolution: { integrity: '2' },
      fullPkgId: 'qar@1.0.0:2',
    },
  })
})

test('lockfileToDepGraph includes resolved link targets when a lockfile directory is supplied', () => {
  const lockfileDir = path.resolve('project')
  const linkTarget = `link:${path.resolve(lockfileDir, '../shared')}` as DepPath
  expect(lockfileToDepGraph({
    lockfileVersion: '9.0',
    importers: {},
    packages: {
      ['parent@1.0.0' as DepPath]: {
        dependencies: {
          child: '1.0.0',
        },
        resolution: { integrity: '0' },
      },
      ['child@1.0.0' as DepPath]: {
        dependencies: {
          linked: 'link:../shared',
        },
        resolution: { integrity: '1' },
      },
    },
  }, undefined, lockfileDir)).toStrictEqual({
    'parent@1.0.0': {
      children: { child: 'child@1.0.0' },
      resolution: { integrity: '0' },
      fullPkgId: 'parent@1.0.0:0',
    },
    'child@1.0.0': {
      children: { linked: linkTarget },
      resolution: { integrity: '1' },
      fullPkgId: 'child@1.0.0:1',
    },
    [linkTarget]: {
      children: {},
      fullPkgId: linkTarget,
    },
  })
})

describe('lockfileToDepGraph link target hashing', () => {
  const parentDepPath = 'parent@1.0.0' as DepPath
  const childDepPath = 'child@1.0.0' as DepPath
  const parentPkgMeta: PkgMeta = {
    depPath: parentDepPath,
    name: 'parent',
    version: '1.0.0',
  }

  function graphWithLink (opts: { alias?: string, lockfileDir: string, target: string }) {
    return lockfileToDepGraph({
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        [parentDepPath]: {
          dependencies: { child: '1.0.0' },
          resolution: { integrity: 'parent-integrity' },
        },
        [childDepPath]: {
          dependencies: { [opts.alias ?? 'linked']: `link:${opts.target}` },
          resolution: { integrity: 'child-integrity' },
        },
      },
    }, undefined, opts.lockfileDir)
  }

  function parentSlot (graph: ReturnType<typeof graphWithLink>): string {
    return calcGraphNodeHash({
      graph,
      cache: {},
      builtDepPaths: new Set(),
      buildRequiredCache: {},
    }, parentPkgMeta)
  }

  test('propagates a changed link target through transitive ancestors', () => {
    const fromA = parentSlot(graphWithLink({
      lockfileDir: path.resolve('project'),
      target: '../linked-a',
    }))
    const fromB = parentSlot(graphWithLink({
      lockfileDir: path.resolve('project'),
      target: '../linked-b',
    }))

    expect(fromA).not.toBe(fromB)
  })

  test('shares a slot when different lockfile directories resolve to the same target', () => {
    const fromA = parentSlot(graphWithLink({
      lockfileDir: path.resolve('workspace/a'),
      target: '../shared',
    }))
    const fromB = parentSlot(graphWithLink({
      lockfileDir: path.resolve('workspace/b'),
      target: '../shared',
    }))

    expect(fromA).toBe(fromB)
  })

  test('includes the dependency alias in the hash', () => {
    const linked = graphWithLink({
      alias: 'linked',
      lockfileDir: path.resolve('project'),
      target: '../shared',
    })
    const renamed = graphWithLink({
      alias: 'renamed',
      lockfileDir: path.resolve('project'),
      target: '../shared',
    })

    expect(parentSlot(linked)).not.toBe(parentSlot(renamed))
  })
})

describe('lockfileToDepGraph link hash parity with pacquet', () => {
  const fixturePackage = linkHashFixture.package
  const cases = process.platform === 'win32' ? linkHashFixture.win32 : linkHashFixture.posix

  test.each(cases)('$name', ({ lockfileDir, target, expectedLinkNode, expectedSlot }) => {
    const graph = lockfileToDepGraph({
      lockfileVersion: '9.0',
      importers: {},
      packages: {
        [fixturePackage.key]: {
          dependencies: { [fixturePackage.alias]: `link:${target}` },
          resolution: { integrity: fixturePackage.integrity },
        },
      },
    }, undefined, lockfileDir)

    expect(graph[fixturePackage.key]?.children[fixturePackage.alias]).toBe(expectedLinkNode)
    expect(calcGraphNodeHash({
      graph,
      cache: {},
      builtDepPaths: new Set(),
      buildRequiredCache: {},
    }, {
      depPath: fixturePackage.key,
      name: fixturePackage.name,
      version: fixturePackage.version,
    })).toBe(expectedSlot)
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
