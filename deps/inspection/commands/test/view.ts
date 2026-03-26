import type { Config } from '@pnpm/config.reader'
import { view } from '@pnpm/deps.inspection.commands'
import { PnpmError } from '@pnpm/error'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const VIEW_OPTIONS = {
  registries: { default: REGISTRY_URL },
}

test('view: command should be available', () => {
  expect(view.handler).toBeDefined()
  expect(view.help).toBeDefined()
  expect(view.commandNames).toBeDefined()
  expect(view.cliOptionsTypes).toBeDefined()
})

test('view: command should have correct names', () => {
  expect(view.commandNames).toEqual(['view', 'info', 'show', 'v'])
})

test('view: help should return a string', () => {
  const help = view.help()
  expect(typeof help).toBe('string')
  expect(help.length).toBeGreaterThan(0)
})

test('view: cliOptionsTypes should return object', () => {
  const types = view.cliOptionsTypes()
  expect(typeof types).toBe('object')
})

test('view: rcOptionsTypes should return object', () => {
  const types = view.rcOptionsTypes()
  expect(typeof types).toBe('object')
})

test('view: missing package name throws error', async () => {
  await expect(
    view.handler(VIEW_OPTIONS as unknown as Config, [])
  ).rejects.toThrow(PnpmError)
})

test('view: successful lookup of package', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config, ['is-negative'])
  expect(typeof result).toBe('string')
  expect(result).toContain('is-negative')
})

test('view: package not found throws an error', async () => {
  await expect(
    view.handler(VIEW_OPTIONS as unknown as Config, ['not-a-real-package-123456789'])
  ).rejects.toThrow(PnpmError)
})

test('view: with --json option', async () => {
  const result = await view.handler({ ...VIEW_OPTIONS, json: true } as unknown as Config, ['is-negative'])
  expect(typeof result).toBe('string')
  const parsed = JSON.parse(result as string)
  expect(parsed.name).toBe('is-negative')
})

test('view: accessing a specific field', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config, ['is-negative', 'name'])
  expect(result).toBe('is-negative')
})

test('view: accessing a specific version', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config, ['is-negative@1.0.0', 'version'])
  expect(result).toBe('1.0.0')
})

test('view: accessing multiple fields adds quotes for strings', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config, ['is-negative@1.0.0', 'name', 'version'])
  expect(typeof result).toBe('string')
  expect(result).toContain("name = 'is-negative'")
  expect(result).toContain("version = '1.0.0'")
})

