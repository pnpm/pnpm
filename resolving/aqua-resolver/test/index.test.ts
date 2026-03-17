import { describe, expect, it } from '@jest/globals'
import type { FetchFromRegistry } from '@pnpm/fetching-types'
import { resolveAqua } from '@pnpm/resolving.aqua-resolver'

const dummyFetch = (() => {}) as unknown as FetchFromRegistry

describe('resolveAqua', () => {
  it('returns null for non-aqua specifiers', async () => {
    const result = await resolveAqua(
      { fetchFromRegistry: dummyFetch },
      { bareSpecifier: 'lodash@4.0.0' }
    )
    expect(result).toBeNull()
  })

  it('throws in offline mode', async () => {
    await expect(
      resolveAqua(
        { fetchFromRegistry: dummyFetch, offline: true },
        { bareSpecifier: 'aqua:BurntSushi/ripgrep' }
      )
    ).rejects.toThrow('Cannot resolve aqua packages in offline mode')
  })

  it('throws for invalid specifier without owner/repo format', async () => {
    await expect(
      resolveAqua(
        { fetchFromRegistry: dummyFetch },
        { bareSpecifier: 'aqua:ripgrep' }
      )
    ).rejects.toThrow('Expected format: aqua:owner/repo[@version]')
  })
})
