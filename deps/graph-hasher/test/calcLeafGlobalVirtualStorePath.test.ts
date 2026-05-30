import { describe, expect, it } from '@jest/globals'
import { calcGlobalVirtualStorePathWithSubdeps, calcLeafGlobalVirtualStorePath } from '@pnpm/deps.graph-hasher'

describe('calcLeafGlobalVirtualStorePath', () => {
  it('returns a stable path for a leaf package', () => {
    const path = calcLeafGlobalVirtualStorePath('foo@1.0.0:sha512-abc', 'foo', '1.0.0')
    expect(path).toMatch(/^@\/foo\/1\.0\.0\/[a-f0-9]+$/)
  })
})

describe('calcGlobalVirtualStorePathWithSubdeps', () => {
  it('equals the leaf path when no subdeps are passed', () => {
    const leafPath = calcLeafGlobalVirtualStorePath('foo@1.0.0:sha512-abc', 'foo', '1.0.0')
    const withEmptySubdeps = calcGlobalVirtualStorePathWithSubdeps('foo@1.0.0:sha512-abc', 'foo', '1.0.0', {})
    expect(withEmptySubdeps).toBe(leafPath)
  })

  it('produces a different path when an optional subdep changes version', () => {
    const fullPkgId = 'foo@1.0.0:sha512-abc'
    const withSubdepV1 = calcGlobalVirtualStorePathWithSubdeps(fullPkgId, 'foo', '1.0.0', {
      'foo-darwin-arm64': 'foo-darwin-arm64@1.0.0:sha512-aaa',
    })
    const withSubdepV2 = calcGlobalVirtualStorePathWithSubdeps(fullPkgId, 'foo', '1.0.0', {
      'foo-darwin-arm64': 'foo-darwin-arm64@1.1.0:sha512-bbb',
    })
    expect(withSubdepV1).not.toBe(withSubdepV2)
  })

  it('produces a different path when an optional subdep is added', () => {
    const fullPkgId = 'foo@1.0.0:sha512-abc'
    const noSubdeps = calcGlobalVirtualStorePathWithSubdeps(fullPkgId, 'foo', '1.0.0', {})
    const withSubdep = calcGlobalVirtualStorePathWithSubdeps(fullPkgId, 'foo', '1.0.0', {
      'foo-darwin-arm64': 'foo-darwin-arm64@1.0.0:sha512-aaa',
    })
    expect(noSubdeps).not.toBe(withSubdep)
  })

  it('is order-independent across multiple subdeps', () => {
    const fullPkgId = 'foo@1.0.0:sha512-abc'
    const orderA = calcGlobalVirtualStorePathWithSubdeps(fullPkgId, 'foo', '1.0.0', {
      'foo-darwin-arm64': 'foo-darwin-arm64@1.0.0:sha512-aaa',
      'foo-linux-x64': 'foo-linux-x64@1.0.0:sha512-bbb',
    })
    const orderB = calcGlobalVirtualStorePathWithSubdeps(fullPkgId, 'foo', '1.0.0', {
      'foo-linux-x64': 'foo-linux-x64@1.0.0:sha512-bbb',
      'foo-darwin-arm64': 'foo-darwin-arm64@1.0.0:sha512-aaa',
    })
    expect(orderA).toBe(orderB)
  })
})
