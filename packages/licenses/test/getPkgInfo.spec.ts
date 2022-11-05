import { getPkgInfo } from '../lib/getPkgInfo'

export const DEFAULT_REGISTRIES = {
  default: 'https://registry.npmjs.org/',
}

describe('licences', () => {
  test('getPkgInfo() should throw error when package info can not be fetched', async () => {
    await expect(
      getPkgInfo(
        {
          name: 'bogus-package',
          version: '1.0.0',
          depPath: '/bogus-package/1.0.0',
          snapshot: {
            resolution: {
              integrity: 'integrity-sha',
            },
          },
          registries: DEFAULT_REGISTRIES,
        },
        {
          storeDir: 'store-dir',
          virtualStoreDir: 'virtual-store-dir',
          modulesDir: 'modules-dir',
          dir: 'workspace-dir',
        }
      )
    ).rejects.toThrow('Failed to find package index file for /bogus-package/1.0.0, please consider running \'pnpm install\'')
  })
})
