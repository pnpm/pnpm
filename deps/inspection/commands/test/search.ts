import type { Config, ConfigContext } from '@pnpm/config.reader'
import { search } from '@pnpm/deps.inspection.commands'

test('search: command should be available', () => {
  expect(search.handler).toBeDefined()
  expect(search.help).toBeDefined()
  expect(search.commandNames).toBeDefined()
  expect(search.cliOptionsTypes).toBeDefined()
})

test('search: command should have correct names', () => {
  expect(search.commandNames).toEqual(['search', 's', 'se', 'find'])
})

test('search: help should return a string', () => {
  const help = search.help()
  expect(typeof help).toBe('string')
  expect(help.length).toBeGreaterThan(0)
})

test('search: cliOptionsTypes should return object', () => {
  const types = search.cliOptionsTypes()
  expect(typeof types).toBe('object')
  expect(types.json).toBe(Boolean)
  expect(types['search-limit']).toBe(Number)
})

test('search: missing query throws error', async () => {
  await expect(
    search.handler({} as unknown as Config & ConfigContext, [])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_MISSING_SEARCH_QUERY' })
})
