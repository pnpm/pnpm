import { getPkgInfo } from '../lib/getPkgInfo'

describe('licences', () => {
  test('getPkgInfo() should throw error when package name is missing', async () => {
    await expect(
      getPkgInfo({
        name: '',
        version: '0.0.0',
        prefix: '.',
      })
    ).rejects.toThrow('Missing package name')
  })

  test('getPkgInfo() should throw error when package info can not be fetched', async () => {
    await expect(
      getPkgInfo({
        name: 'bogus-package',
        version: '0.0.0',
        prefix: 'package-prefix',
      })
    ).rejects.toThrow('Failed to fetch manifest data for bogus-package')
  })
})
