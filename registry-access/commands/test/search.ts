import { expect, test } from '@jest/globals'
import { search } from '@pnpm/registry-access.commands'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { DEFAULT_OPTS } from '@pnpm/testing.command-defaults'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const SEARCH_OPTS = {
  ...DEFAULT_OPTS,
  registries: { default: `${REGISTRY_URL}/` },
}

test('search: missing query throws error', async () => {
  await expect(
    search.handler(SEARCH_OPTS, [])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_MISSING_SEARCH_QUERY' })
})

test('search: returns formatted output with package name and npmx.dev URL', async () => {
  const result = await search.handler(SEARCH_OPTS, ['is-positive'])
  expect(typeof result).toBe('string')
  expect(result).toContain('is-positive')
  expect(result).toContain('Version ')
  expect(result).toContain('https://npmx.dev/package/is-positive')
  expect(result).not.toContain('npm.im')
})

test('search: --json returns parsed package array', async () => {
  const result = await search.handler({ ...SEARCH_OPTS, json: true }, ['is-positive'])
  const parsed = JSON.parse(result)
  expect(Array.isArray(parsed)).toBe(true)
  expect(parsed.length).toBeGreaterThan(0)
  expect(parsed[0].name).toBeDefined()
  expect(parsed[0].version).toBeDefined()
})

test('search: empty results returns "No packages found"', async () => {
  const result = await search.handler(SEARCH_OPTS, ['this-package-definitely-does-not-exist-xyz-123456789'])
  expect(result).toBe('No packages found')
})

test('search: non-OK registry response throws SEARCH_FAILED', async () => {
  await expect(
    search.handler({
      ...SEARCH_OPTS,
      registries: { default: `${REGISTRY_URL}/nonexistent-registry-path/` },
    }, ['is-positive'])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_SEARCH_FAILED' })
})

test('search: command metadata', () => {
  expect(search.commandNames).toEqual(['search', 's', 'se', 'find'])
  const types = search.cliOptionsTypes()
  expect(types.json).toBe(Boolean)
  expect(types['search-limit']).toBe(Number)
})
