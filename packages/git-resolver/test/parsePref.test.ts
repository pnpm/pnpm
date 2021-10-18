import parsePref from '@pnpm/git-resolver/lib/parsePref'

test.each([
  ['ssh://username:password@example.com:repo.git', 'ssh://username:password@example.com/repo.git'],
  ['ssh://username:password@example.com:repo/@foo.git', 'ssh://username:password@example.com/repo/@foo.git'],
])('the right colon is escaped', async (input, output) => {
  const parsed = await parsePref(input)
  expect(parsed?.fetchSpec).toBe(output)
})
