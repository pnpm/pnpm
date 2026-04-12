import { jest } from '@jest/globals'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const mockOpen = jest.fn()
jest.unstable_mockModule('open', () => ({
  default: mockOpen,
}))

const { docs } = await import('@pnpm/deps.inspection.commands')

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const DOCS_OPTIONS = {
  registries: { default: REGISTRY_URL },
}

test('docs: command should be available', () => {
  expect(docs.handler).toBeDefined()
  expect(docs.help).toBeDefined()
  expect(docs.commandNames).toBeDefined()
  expect(docs.cliOptionsTypes).toBeDefined()
})

test('docs: command should have correct names', () => {
  expect(docs.commandNames).toEqual(['docs', 'home'])
})

test('docs: help should return a string', () => {
  const help = docs.help()
  expect(typeof help).toBe('string')
  expect(help.length).toBeGreaterThan(0)
})

test('docs: cliOptionsTypes should return object', () => {
  const types = docs.cliOptionsTypes()
  expect(typeof types).toBe('object')
})

test('docs: rcOptionsTypes should return object', () => {
  const types = docs.rcOptionsTypes()
  expect(typeof types).toBe('object')
})

test('docs: missing package name throws error', async () => {
  await expect(
    docs.handler(DOCS_OPTIONS as unknown as Config & ConfigContext, [])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_MISSING_PACKAGE_NAME' })
})

test('docs: successful lookup of package opens documentation', async () => {
  mockOpen.mockClear()
  await docs.handler(DOCS_OPTIONS as unknown as Config & ConfigContext, ['is-negative'])
  expect(mockOpen).toHaveBeenCalled()
  const calledUrl = mockOpen.mock.calls[0][0]
  expect(calledUrl).toBe('https://github.com/kevva/is-negative#readme')
})
