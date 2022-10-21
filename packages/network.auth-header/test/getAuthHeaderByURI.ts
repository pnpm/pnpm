import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'

test('getAuthHeaderByURI()', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    allSettings: {
      '//reg.com/:_authToken': 'abc123',
      '//reg.co/tarballs/:_authToken': 'xxx',
    },
    userSettings: {},
  })
  expect(getAuthHeaderByURI('https://reg.com/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.com/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.io/foo/-/foo-1.0.0.tgz')).toBe(undefined)
  expect(getAuthHeaderByURI('https://reg.co/tarballs/foo/-/foo-1.0.0.tgz')).toBe('Bearer xxx')
})

test('returns undefined when the auth header is not found', () => {
  expect(createGetAuthHeaderByURI({ allSettings: {}, userSettings: {} })('http://reg.com')).toBe(undefined)
})
