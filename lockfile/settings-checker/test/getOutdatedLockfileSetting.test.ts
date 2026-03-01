import { type LockfileObject } from '@pnpm/lockfile.types'
import { getOutdatedLockfileSetting } from '@pnpm/lockfile.settings-checker'

const DEFAULT_OPTS = {
  autoInstallPeers: true,
  excludeLinksFromLockfile: false,
  peersSuffixMaxLength: 1000,
}

function createLockfile (overrides?: Partial<LockfileObject>): LockfileObject {
  return {
    lockfileVersion: '9.0',
    importers: {},
    settings: {
      autoInstallPeers: true,
      excludeLinksFromLockfile: false,
      peersSuffixMaxLength: 1000,
    },
    ...overrides,
  }
}

describe('getOutdatedLockfileSetting', () => {
  describe('catalogs', () => {
    test('returns null when catalogs is undefined and lockfile has catalogs (no workspace)', () => {
      const lockfile = createLockfile({
        catalogs: {
          default: {
            'is-odd': { specifier: '^3.0.1', version: '3.0.1' },
          },
        },
      })
      expect(getOutdatedLockfileSetting(lockfile, {
        ...DEFAULT_OPTS,
        catalogs: undefined,
      })).toBeNull()
    })

    test('returns null when catalogs match', () => {
      const lockfile = createLockfile({
        catalogs: {
          default: {
            'is-odd': { specifier: '^3.0.1', version: '3.0.1' },
          },
        },
      })
      expect(getOutdatedLockfileSetting(lockfile, {
        ...DEFAULT_OPTS,
        catalogs: { default: { 'is-odd': '^3.0.1' } },
      })).toBeNull()
    })

    test('returns "catalogs" when catalogs do not match', () => {
      const lockfile = createLockfile({
        catalogs: {
          default: {
            'is-odd': { specifier: '^3.0.1', version: '3.0.1' },
          },
        },
      })
      expect(getOutdatedLockfileSetting(lockfile, {
        ...DEFAULT_OPTS,
        catalogs: { default: { 'is-odd': '^4.0.0' } },
      })).toBe('catalogs')
    })

    test('returns null when both catalogs and lockfile catalogs are empty', () => {
      const lockfile = createLockfile()
      expect(getOutdatedLockfileSetting(lockfile, {
        ...DEFAULT_OPTS,
        catalogs: {},
      })).toBeNull()
    })
  })
})
