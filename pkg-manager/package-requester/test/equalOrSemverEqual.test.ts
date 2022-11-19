import { equalOrSemverEqual } from '../lib/equalOrSemverEqual'

test('equalOrSemverEqual()', () => {
  expect(equalOrSemverEqual('a', 'a')).toBeTruthy()
  expect(equalOrSemverEqual('a', 'b')).toBeFalsy()
  expect(equalOrSemverEqual('1.0.0', 'v1.0.0')).toBeTruthy()
})
