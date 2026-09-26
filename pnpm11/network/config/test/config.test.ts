import { describe, expect, test } from '@jest/globals'
import { pickSettingByUrl } from '@pnpm/network.config'

describe('pickSettingByUrl', () => {
  test('should return undefined if generic object is undefined', () => {
    expect(pickSettingByUrl(undefined, 'https://example.com')).toBeUndefined()
  })

  test('should return the exact match from the generic object', () => {
    const settings = { 'https://example.com/': 'ExampleSetting' }
    expect(pickSettingByUrl(settings, 'https://example.com')).toBe(
      'ExampleSetting'
    )
  })

  test('should return a match using nerf dart', () => {
    const settings = { '//example.com/': 'NerfDartSetting' }
    expect(
      pickSettingByUrl(settings, 'https://example.com/path/to/resource')
    ).toBe('NerfDartSetting')
  })

  test('should return a match using withoutPort', () => {
    const settings = {
      'https://example.com/path/to/resource/': 'WithoutPortSetting',
    }
    expect(
      pickSettingByUrl(settings, 'https://example.com:8080/path/to/resource')
    ).toBe('WithoutPortSetting')
  })

  test('should return undefined if no match is found', () => {
    const settings = { 'https://example.com/': 'ExampleSetting' }
    expect(pickSettingByUrl(settings, 'https://nomatch.com')).toBeUndefined()
  })

  test('should recursively match using withoutPort', () => {
    const settings = { 'https://example.com/': 'RecursiveSetting' }
    expect(pickSettingByUrl(settings, 'https://example.com:8080')).toBe(
      'RecursiveSetting'
    )
  })
})
