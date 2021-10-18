import parsePref from '@pnpm/git-resolver/lib/parsePref'

test('the right colon is escaped', async () => {
  const parsed = await parsePref('ssh://username:password@example.com:repo.git')
  expect(parsed?.fetchSpec).toBe('ssh://username:password@example.com/repo.git')
})
