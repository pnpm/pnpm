import { jsrToNpmPackageName } from '../src/string'

test('jsrToNpmPackageName', () => {
  expect(jsrToNpmPackageName('@foo/bar')).toBe('@jsr/foo__bar')
})
