import { parsePref } from '../lib/parsePref'

test.each([
  ['ssh://username:password@example.com:repo.git', 'ssh://username:password@example.com/repo.git'],
  ['ssh://username:password@example.com:repo/@foo.git', 'ssh://username:password@example.com/repo/@foo.git'],
  ['ssh://username:password@example.com:22/repo/@foo.git', 'ssh://username:password@example.com:22/repo/@foo.git'],
  ['ssh://username:password@example.com:22repo/@foo.git', 'ssh://username:password@example.com/22repo/@foo.git'],
  ['git+ssh://username:password@example.com:repo.git', 'ssh://username:password@example.com/repo.git'],
  ['git+ssh://username:password@example.com:repo/@foo.git', 'ssh://username:password@example.com/repo/@foo.git'],
  ['git+ssh://username:password@example.com:22/repo/@foo.git', 'ssh://username:password@example.com:22/repo/@foo.git'],
])('the right colon is escaped in %s', async (input, output) => {
  const parsed = await parsePref(input)
  expect(parsed?.fetchSpec).toBe(output)
})
