import { shouldRunCheck } from '../src/shouldRunCheck'

test('should return true if npm_lifecycle_event is not defined', () => {
  expect(shouldRunCheck({})).toBe(true)
})

describe('should return false if npm_lifecycle_event is an install hook', () => {
  test.each([
    'preinstall',
    'install',
    'postinstall',
    'preuninstall',
    'uninstall',
    'postuninstall',
  ])('%s', value => {
    expect(shouldRunCheck({ npm_lifecycle_event: value })).toBe(false)
  })
})

describe('should return true if npm_lifecycle_event is not an install hook', () => {
  test.each([
    'test',
    'build',
    'anything',
  ])('%s', value => {
    expect(shouldRunCheck({ npm_lifecycle_event: value })).toBe(true)
  })
})
