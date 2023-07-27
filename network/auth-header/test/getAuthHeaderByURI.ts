import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'

test('getAuthHeaderByURI()', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    allSettings: {
      '//reg.com/:_authToken': 'abc123',
      '//reg.co/tarballs/:_authToken': 'xxx',
      '//reg.gg:8888/:_authToken': '0000',
    },
    userSettings: {},
  })
  expect(getAuthHeaderByURI('https://reg.com/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.com/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.com:8080/foo/-/foo-1.0.0.tgz')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('https://reg.io/foo/-/foo-1.0.0.tgz')).toBe(undefined)
  expect(getAuthHeaderByURI('https://reg.co/tarballs/foo/-/foo-1.0.0.tgz')).toBe('Bearer xxx')
  expect(getAuthHeaderByURI('https://reg.gg:8888/foo/-/foo-1.0.0.tgz')).toBe('Bearer 0000')
})

test('getAuthHeaderByURI() when default ports are specified', () => {
  const getAuthHeaderByURI = createGetAuthHeaderByURI({
    allSettings: {
      '//reg.com/:_authToken': 'abc123',
    },
    userSettings: {},
  })
  expect(getAuthHeaderByURI('https://reg.com:443/')).toBe('Bearer abc123')
  expect(getAuthHeaderByURI('http://reg.com:80/')).toBe('Bearer abc123')
})

test('returns undefined when the auth header is not found', () => {
  expect(createGetAuthHeaderByURI({ allSettings: {}, userSettings: {} })('http://reg.com')).toBe(undefined)
})
