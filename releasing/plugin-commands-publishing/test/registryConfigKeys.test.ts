import { type RegistryConfigKey, allRegistryConfigKeys, longestRegistryConfigKey } from '../src/registryConfigKeys.js'

describe('longestRegistryConfigKey', () => {
  type Case = [string, RegistryConfigKey | undefined]
  test.each([
    ['https://example.com/foo/bar/', '//example.com/foo/bar/'],
    ['https://example.com/foo/bar', '//example.com/foo/bar/'],
    ['http://example.com/foo/bar/', '//example.com/foo/bar/'],
    ['http://example.com/foo/bar', '//example.com/foo/bar/'],
    ['https://example.com/', '//example.com/'],
    ['https://example.com', '//example.com/'],
    ['http://example.com/', '//example.com/'],
    ['http://example.com', '//example.com/'],
    ['ftp://example.com/', undefined],
    ['sftp://example.com/', undefined],
    ['file:///example.tgz', undefined],
  ] as Case[])('%p → %p', (registryUrl, registryConfigKey) => {
    expect(longestRegistryConfigKey(registryUrl)).toBe(registryConfigKey)
  })
})

describe('allRegistryConfigKeys', () => {
  test('lists all keys from longest to shortest', () => {
    expect(Array.from(allRegistryConfigKeys('//example.com/foo/bar/'))).toStrictEqual([
      '//example.com/foo/bar/',
      '//example.com/foo/',
      '//example.com/',
    ])
  })

  test('rejects keys without hostname', () => {
    expect(() => allRegistryConfigKeys('///').next()).toThrow(new RangeError('Registry config key cannot be without hostname'))
  })

  test('rejects keys that do not start with double slash', () => {
    expect(
      () => allRegistryConfigKeys('https://example.com' as RegistryConfigKey).next()
    ).toThrow(new RangeError('The string "https://example.com" is not a valid registry config key'))
    expect(
      () => allRegistryConfigKeys('' as RegistryConfigKey).next()
    ).toThrow(new RangeError('The string "" is not a valid registry config key'))
  })
})
