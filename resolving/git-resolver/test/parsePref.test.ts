import { parseBareSpecifier } from '../lib/parseBareSpecifier'

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
  const parsed = await parseBareSpecifier(input, {})
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
  const parsed = await parseBareSpecifier(input, {})
  expect(parsed?.path).toBe(output)
})

test.each([
  ['git+https://github.com/pnpm/pnpm.git', 'https://github.com/pnpm/pnpm.git'],
  ['git+ssh://git@sub.domain.tld:internal-app/sub-path/service-name.git', 'ssh://git@sub.domain.tld/internal-app/sub-path/service-name.git'],
])('the fetchSpec of %s should be %s', async (input, output) => {
  const parsed = await parseBareSpecifier(input, {})
  expect(parsed?.fetchSpec).toBe(output)
})
