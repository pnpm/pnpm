import { expect, test } from '@jest/globals'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { view } from '@pnpm/deps.inspection.commands'
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
    view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, [])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_MISSING_PACKAGE_NAME' })
})

test('view: non-registry spec throws error', async () => {
  await expect(
    view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['github:user/repo'])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_PACKAGE_NAME' })
})

test('view: successful lookup of package', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative'])
  expect(typeof result).toBe('string')
  expect(result).toContain('is-negative')
})

test('view: package not found throws an error', async () => {
  await expect(
    view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['not-a-real-package-123456789'])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_FETCH_404' })
})

test('view: no matching version throws an error', async () => {
  await expect(
    view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@99999.0.0'])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_PACKAGE_NOT_FOUND' })
})

test('view: with --json option', async () => {
  const result = await view.handler({ ...VIEW_OPTIONS, json: true } as unknown as Config & ConfigContext, ['is-negative'])
  expect(typeof result).toBe('string')
  const parsed = JSON.parse(result as string)
  expect(parsed.name).toBe('is-negative')
})

test('view: accessing a specific field', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative', 'name'])
  expect(result).toBe('is-negative')
})

test('view: accessing a specific version', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0', 'version'])
  expect(result).toBe('1.0.0')
})

test('view: accessing multiple fields adds quotes for strings', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0', 'name', 'version'])
  expect(typeof result).toBe('string')
  expect(result).toContain("name = 'is-negative'")
  expect(result).toContain("version = '1.0.0'")
})

test('view: version range resolves to matching version', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@^1.0.0', 'version'])
  expect(typeof result).toBe('string')
  expect(result).toMatch(/^1\./)
})

test('view: dist-tag resolves correctly', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@latest', 'version'])
  expect(typeof result).toBe('string')
  expect(result).toMatch(/^\d+\.\d+\.\d+/)
})

test('view: nested field selection', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0', 'dist.shasum'])
  expect(typeof result).toBe('string')
  expect(result!.length).toBeGreaterThan(0)
})

test('view: field selection with --json', async () => {
  const result = await view.handler(
    { ...VIEW_OPTIONS, json: true } as unknown as Config & ConfigContext,
    ['is-negative@1.0.0', 'name', 'version']
  )
  const parsed = JSON.parse(result as string)
  expect(parsed.name).toBe('is-negative')
  expect(parsed.version).toBe('1.0.0')
})

test('view: text output includes header with name@version', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0']) as string
  const firstLine = result.split('\n')[0]
  expect(firstLine).toContain('is-negative@1.0.0')
})

test('view: text output includes dist section', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0']) as string
  expect(result).toContain('.tarball:')
  expect(result).toContain('.shasum:')
})

test('view: text output includes dist-tags', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative']) as string
  expect(result).toContain('dist-tags:')
  expect(result).toContain('latest:')
})

test('view: text output for package with dependencies shows deps count', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['@pnpm.e2e/pkg-with-1-dep@100.0.0']) as string
  const firstLine = result.split('\n')[0]
  expect(firstLine).toContain('deps: ')
  expect(firstLine).not.toContain('deps: none')
})

test('view: text output for package without dependencies shows deps: none', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0']) as string
  const firstLine = result.split('\n')[0]
  expect(firstLine).toContain('deps: none')
})

test('view: scoped package lookup', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['@pnpm.e2e/pkg-with-1-dep@100.0.0', 'name'])
  expect(result).toBe('@pnpm.e2e/pkg-with-1-dep')
})

test('view: object field renders as JSON', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0', 'dist'])
  expect(typeof result).toBe('string')
  const parsed = JSON.parse(result as string)
  expect(parsed.tarball).toBeDefined()
  expect(parsed.shasum).toBeDefined()
})

test('view: versions field returns array of version strings', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative', 'versions'])
  expect(typeof result).toBe('string')
  const parsed = JSON.parse(result as string)
  expect(Array.isArray(parsed)).toBe(true)
  expect(parsed.length).toBeGreaterThan(0)
  expect(parsed).toContain('1.0.0')
})

test('view: versions field with --json returns raw array', async () => {
  const result = await view.handler(
    { ...VIEW_OPTIONS, json: true } as unknown as Config & ConfigContext,
    ['is-negative', 'versions']
  )
  const parsed = JSON.parse(result as string)
  expect(Array.isArray(parsed)).toBe(true)
  expect(parsed).toContain('1.0.0')
})

test('view: single field with --json returns unwrapped value', async () => {
  const result = await view.handler(
    { ...VIEW_OPTIONS, json: true } as unknown as Config & ConfigContext,
    ['is-negative@1.0.0', 'name']
  )
  const parsed = JSON.parse(result as string)
  expect(parsed).toBe('is-negative')
})

test('view: dist-tags field returns tag-to-version mapping', async () => {
  const result = await view.handler(
    { ...VIEW_OPTIONS, json: true } as unknown as Config & ConfigContext,
    ['is-negative', 'dist-tags']
  )
  const parsed = JSON.parse(result as string)
  expect(typeof parsed).toBe('object')
  expect(parsed.latest).toBeDefined()
})

test('view: time field returns publish timestamps', async () => {
  const result = await view.handler(
    { ...VIEW_OPTIONS, json: true } as unknown as Config & ConfigContext,
    ['is-negative', 'time']
  )
  const parsed = JSON.parse(result as string)
  expect(typeof parsed).toBe('object')
  expect(parsed['1.0.0']).toBeDefined()
})
