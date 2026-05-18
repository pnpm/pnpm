import { describe, expect, it } from '@jest/globals'
import { calcLeafGlobalVirtualStorePath } from '@pnpm/deps.graph-hasher'

describe('calcLeafGlobalVirtualStorePath', () => {
  it('returns the same path when deps is omitted vs. passed as empty', () => {
    const withoutDeps = calcLeafGlobalVirtualStorePath('foo@1.0.0:sha512-abc', 'foo', '1.0.0')
    const withEmptyDeps = calcLeafGlobalVirtualStorePath('foo@1.0.0:sha512-abc', 'foo', '1.0.0', {})
    expect(withoutDeps).toBe(withEmptyDeps)
  })

  it('produces a different hash when an optional subdep changes version', () => {
    const fullPkgId = 'foo@1.0.0:sha512-abc'
    const withSubdepV1 = calcLeafGlobalVirtualStorePath(fullPkgId, 'foo', '1.0.0', {
      'foo-darwin-arm64': 'foo-darwin-arm64@1.0.0:sha512-aaa',
    })
    const withSubdepV2 = calcLeafGlobalVirtualStorePath(fullPkgId, 'foo', '1.0.0', {
      'foo-darwin-arm64': 'foo-darwin-arm64@1.1.0:sha512-bbb',
    })
    expect(withSubdepV1).not.toBe(withSubdepV2)
  })

  it('produces a different hash when an optional subdep is added', () => {
    const fullPkgId = 'foo@1.0.0:sha512-abc'
    const noSubdeps = calcLeafGlobalVirtualStorePath(fullPkgId, 'foo', '1.0.0')
    const withSubdep = calcLeafGlobalVirtualStorePath(fullPkgId, 'foo', '1.0.0', {
      'foo-darwin-arm64': 'foo-darwin-arm64@1.0.0:sha512-aaa',
    })
    expect(noSubdeps).not.toBe(withSubdep)
  })

  it('is order-independent across multiple subdeps', () => {
    const fullPkgId = 'foo@1.0.0:sha512-abc'
    const orderA = calcLeafGlobalVirtualStorePath(fullPkgId, 'foo', '1.0.0', {
      'foo-darwin-arm64': 'foo-darwin-arm64@1.0.0:sha512-aaa',
      'foo-linux-x64': 'foo-linux-x64@1.0.0:sha512-bbb',
    })
    const orderB = calcLeafGlobalVirtualStorePath(fullPkgId, 'foo', '1.0.0', {
      'foo-linux-x64': 'foo-linux-x64@1.0.0:sha512-bbb',
      'foo-darwin-arm64': 'foo-darwin-arm64@1.0.0:sha512-aaa',
    })
    expect(orderA).toBe(orderB)
  })
})
