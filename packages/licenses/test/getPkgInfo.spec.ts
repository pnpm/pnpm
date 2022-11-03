import { PackageSnapshot } from '@pnpm/lockfile-file'
import { getPkgInfo } from '../lib/getPkgInfo'

describe('licences', () => {
  test('getPkgInfo() should throw error when package info can not be fetched', async () => {
    await expect(
      getPkgInfo(
        {
          name: 'bogus-package',
          version: '0.0.0',
          depPath: 'dep-path',
          snapshot: {} as PackageSnapshot,
        },
        {
          storeDir: 'store-dir',
          virtualStoreDir: 'virtual-store-dir',
          modulesDir: 'modules-dir',
          dir: 'workspace-dir',
        }
      )
    ).rejects.toThrow('Failed to fetch manifest data')
  })
})
