import { shouldRunCheck } from '../src/shouldRunCheck'

test('should return true if npm_lifecycle_event is not defined', () => {
  expect(shouldRunCheck({})).toBe(true)
})

test('should return false if npm_lifecycle_event is defined', () => {
  expect(shouldRunCheck({
    npm_lifecycle_event: 'install',
  })).toBe(false)
})
