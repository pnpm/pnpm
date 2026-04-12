import type { Config, ConfigContext } from '@pnpm/config.reader'
import { search } from '@pnpm/deps.inspection.commands'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const SEARCH_OPTIONS = {
  registries: { default: REGISTRY_URL },
  searchLimit: 20,
}

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

test.skip('search: successful search returns table output', async () => {
  const result = await search.handler(SEARCH_OPTIONS as unknown as Config & ConfigContext, ['hello-world'])
  expect(typeof result).toBe('string')
  expect(result).toContain('hello-world')
})

test.skip('search: json output returns array', async () => {
  const result = await search.handler({ ...SEARCH_OPTIONS, json: true } as unknown as Config & ConfigContext, ['hello-world'])
  expect(typeof result).toBe('string')
  const parsed = JSON.parse(result)
  expect(Array.isArray(parsed)).toBe(true)
  expect(parsed[0]?.name).toBeDefined()
})

test.skip('search: custom registry URL works', async () => {
  const customRegistry = REGISTRY_URL + '/npm/'
  const result = await search.handler({
    registries: { default: customRegistry },
    searchLimit: 5,
  } as unknown as Config & ConfigContext, ['hello-world'])
  expect(typeof result).toBe('string')
})
