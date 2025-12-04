import { normalizeRegistryUrl } from '../lib/normalizeRegistryUrl.js'

test.each([
  ['https://registry.example.com:443/package.tgz', 'https://registry.example.com/package.tgz'],
  ['http://registry.example.com:80/package.tgz', 'http://registry.example.com/package.tgz'],
  ['https://registry.example.com:8443/package.tgz', 'https://registry.example.com:8443/package.tgz'],
  ['http://registry.example.com:8080/package.tgz', 'http://registry.example.com:8080/package.tgz'],
  ['https://registry.example.com/package.tgz', 'https://registry.example.com/package.tgz'],
  ['http://registry.example.com/package.tgz', 'http://registry.example.com/package.tgz'],
  ['https://artifactory:443/api/npm/npm-virtual/uuid/-/uuid-9.0.1.tgz', 'https://artifactory/api/npm/npm-virtual/uuid/-/uuid-9.0.1.tgz'],
  ['invalid-url', 'invalid-url'],
])('normalizeRegistryUrl(%s) should return %s', (input, expected) => {
  expect(normalizeRegistryUrl(input)).toBe(expected)
})
