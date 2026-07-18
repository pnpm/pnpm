import { describe, expect, it } from '@jest/globals'
import { calcGlobalVirtualStorePathWithSubdeps, calcLeafGlobalVirtualStorePath } from '@pnpm/deps.graph-hasher'

describe('calcLeafGlobalVirtualStorePath', () => {
  it('returns a stable path for a leaf package', () => {
    const path = calcLeafGlobalVirtualStorePath('foo@1.0.0:sha512-abc', 'foo', '1.0.0')
    expect(path).toMatch(/^@\/foo\/1\.0\.0\/[a-f0-9]+$/)
  })

  // The version segment is lockfile-controlled and inserted raw into the slot
  // path; a `..` segment would let the slot escape the global virtual store
  // root once joined onto `globalVirtualStoreDir`. All GVS slot builders funnel
  // through `formatGlobalVirtualStorePath`, so the rejection happens here.
  it.each(['../../../escape', '..', 'a/../../b', 'x/..'])(
    'rejects a traversal version segment %p',
    (version) => {
      expect(() => calcLeafGlobalVirtualStorePath('foo@1.0.0:sha512-abc', 'foo', version))
        .toThrow(expect.objectContaining({ code: 'ERR_PNPM_INVALID_DEPENDENCY_NAME' }))
    }
  )
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
