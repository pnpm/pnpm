import { expect, jest, test } from '@jest/globals'

jest.unstable_mockModule('@pnpm/network.fetch', () => ({
  fetchWithDispatcher: jest.fn(async () => ({ ok: true })),
}))
jest.unstable_mockModule('execa', () => ({
  safeExeca: jest.fn(async () => ({ stdout: '' })),
}))
const { parseBareSpecifier } = await import('../lib/parseBareSpecifier.js')

test.each([
  ['ssh://username:password@example.com:repo.git', 'ssh://username:password@example.com/repo.git'],
  ['ssh://username:password@example.com:repo/@foo.git', 'ssh://username:password@example.com/repo/@foo.git'],
  ['ssh://username:password@example.com:22/repo/@foo.git', 'ssh://username:password@example.com:22/repo/@foo.git'],
  ['ssh://username:password@example.com:22repo/@foo.git', 'ssh://username:password@example.com/22repo/@foo.git'],
  ['ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b', 'ssh://username:password@example.com:22/repo/@foo.git'],
  ['ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b&dev', 'ssh://username:password@example.com:22/repo/@foo.git'],
  ['git+ssh://username:password@example.com:repo.git', 'ssh://username:password@example.com/repo.git'],
  ['git+ssh://username:password@example.com:repo/@foo.git', 'ssh://username:password@example.com/repo/@foo.git'],
  ['git+ssh://username:password@example.com:22/repo/@foo.git', 'ssh://username:password@example.com:22/repo/@foo.git'],
  ['git+ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b', 'ssh://username:password@example.com:22/repo/@foo.git'],
  ['git+ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b&dev', 'ssh://username:password@example.com:22/repo/@foo.git'],
])('the right colon is escaped in %s', async (input, output) => {
  const parsed = await parseBareSpecifier(input, {})?.()
  expect(parsed?.fetchSpec).toBe(output)
})

test.each([
  ['ssh://username:password@example.com:repo.git#path:/a/@b', '/a/@b'],
  ['ssh://username:password@example.com:repo/@foo.git#path:/a/@b', '/a/@b'],
  ['ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b', '/a/@b'],
  ['ssh://username:password@example.com:22repo/@foo.git#path:/a/@b', '/a/@b'],
  ['ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b', '/a/@b'],
  ['ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b&dev', '/a/@b'],
  ['git+ssh://username:password@example.com:repo.git#path:/a/@b', '/a/@b'],
  ['git+ssh://username:password@example.com:repo/@foo.git#path:/a/@b', '/a/@b'],
  ['git+ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b', '/a/@b'],
  ['git+ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b', '/a/@b'],
  ['git+ssh://username:password@example.com:22/repo/@foo.git#path:/a/@b&dev', '/a/@b'],
  ['ssh://username:password@example.com:repo.git', undefined],
  ['ssh://username:password@example.com:repo/@foo.git', undefined],
  ['ssh://username:password@example.com:22/repo/@foo.git', undefined],
  ['ssh://username:password@example.com:22repo/@foo.git', undefined],
  ['ssh://username:password@example.com:22/repo/@foo.git', undefined],
  ['ssh://username:password@example.com:22/repo/@foo.git#dev', undefined],
  ['git+ssh://username:password@example.com:repo.git', undefined],
  ['git+ssh://username:password@example.com:repo/@foo.git', undefined],
  ['git+ssh://username:password@example.com:22/repo/@foo.git', undefined],
  ['git+ssh://username:password@example.com:22/repo/@foo.git', undefined],
  ['git+ssh://username:password@example.com:22/repo/@foo.git#dev', undefined],
])('the path of %s should be %s', async (input, output) => {
  const parsed = await parseBareSpecifier(input, {})?.()
  expect(parsed?.path).toBe(output)
})

test.each([
  ['git+https://github.com/pnpm/pnpm.git', 'https://github.com/pnpm/pnpm.git'],
  ['git+ssh://git@sub.domain.tld:internal-app/sub-path/service-name.git', 'ssh://git@sub.domain.tld/internal-app/sub-path/service-name.git'],
])('the fetchSpec of %s should be %s', async (input, output) => {
  const parsed = await parseBareSpecifier(input, {})?.()
  expect(parsed?.fetchSpec).toBe(output)
})

test.each([
  ['ssh://git.example.com/team/repo.git', 'ssh://git.example.com/team/repo.git'],
  ['ssh://git.example.com:2222/team/repo.git', 'ssh://git.example.com:2222/team/repo.git'],
  ['ssh://git.example.com:team/repo.git', 'ssh://git.example.com/team/repo.git'],
  ['ssh://git.example.com:repo.git', 'ssh://git.example.com/repo.git'],
  ['git+ssh://git.example.com/team/repo.git', 'ssh://git.example.com/team/repo.git'],
  ['git+ssh://git.example.com:team/repo.git', 'ssh://git.example.com/team/repo.git'],
  ['ssh://git.example.com:2222/team/repo.git#v1.0.0', 'ssh://git.example.com:2222/team/repo.git'],
])('the fetchSpec of %s, which carries no user info, should be %s', async (input, output) => {
  const parsed = await parseBareSpecifier(input, {})?.()
  expect(parsed?.fetchSpec).toBe(output)
})

test.each([
  ['ssh://user@a@b.example.com/repo.git', 'ssh://user%40a@b.example.com/repo.git'],
  ['ssh://user:p@ss@example.com:repo.git', 'ssh://user:p%40ss@example.com/repo.git'],
  ['ssh://user:p@ss@example.com:22/repo.git', 'ssh://user:p%40ss@example.com:22/repo.git'],
])('the fetchSpec of %s, whose authority holds more than one @, should be %s', async (input, output) => {
  const parsed = await parseBareSpecifier(input, {})?.()
  expect(parsed?.fetchSpec).toBe(output)
})

test.each([
  ['ssh://[::1]/repo.git', 'ssh://[::1]/repo.git'],
  ['ssh://[2001:db8::1]/team/repo.git', 'ssh://[2001:db8::1]/team/repo.git'],
  ['ssh://[::1]:2222/repo.git', 'ssh://[::1]:2222/repo.git'],
  ['ssh://[::1]:team/repo.git', 'ssh://[::1]/team/repo.git'],
  ['ssh://git@[::1]/repo.git', 'ssh://git@[::1]/repo.git'],
  ['ssh://git@[::1]:team/repo.git', 'ssh://git@[::1]/team/repo.git'],
])('the fetchSpec of %s, which holds a bracketed IPv6 host, should be %s', async (input, output) => {
  const parsed = await parseBareSpecifier(input, {})?.()
  expect(parsed?.fetchSpec).toBe(output)
})

// Test for https:// URLs ending in .git (issue #10468)
test.each([
  ['https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git', 'https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git'],
  ['https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git#6daccff309fca1e7561a43984d42fa4f829ce06d', 'https://gitea.osmocom.org/ttcn3/highlightjs-ttcn3.git'],
  ['http://example.com/repo.git', 'http://example.com/repo.git'],
  ['http://example.com/repo.git#main', 'http://example.com/repo.git'],
])('plain http/https URLs ending in .git should be recognized: %s', async (input, output) => {
  const parsed = await parseBareSpecifier(input, {})?.()
  expect(parsed?.fetchSpec).toBe(output)
})

// Ensure non-.git https URLs are not recognized as git repos
test.each([
  ['https://example.com/package.tar.gz'],
  ['https://example.com/package.tgz'],
  ['https://example.com/file'],
])('plain http/https URLs not ending in .git should not be recognized: %s', async (input) => {
  const parsed = parseBareSpecifier(input, {})
  expect(parsed).toBeNull()
})
