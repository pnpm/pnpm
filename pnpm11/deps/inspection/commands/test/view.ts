import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { stripVTControlCharacters as stripAnsi } from 'node:util'

import { expect, test } from '@jest/globals'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { view } from '@pnpm/deps.inspection.commands'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'

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
  const cwd = process.cwd()
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'view-test-'))

  try {
    process.chdir(tmpDir)
    await expect(
      view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_MISSING_PACKAGE_NAME' })
  } finally {
    process.chdir(cwd)
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
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

test('view: text output includes bin from object', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['@pnpm.e2e/touch-file-one-bin@1.0.0']) as string
  expect(result).toMatch(/^bin: t/m)
})

test('view: text output includes bin from string', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['@pnpm.e2e/hello-world-js-bin']) as string
  expect(result).toMatch(/^bin: hello-world-js-bin/m)
})

test('view: text output includes dist section', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0']) as string
  expect(result).toContain('.tarball:')
  expect(result).toContain('.shasum:')
})

test('view: text output includes dist-tags', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative']) as string
  const plainTextResult = stripAnsi(result)
  expect(plainTextResult).toContain('dist-tags:')
  expect(plainTextResult).toContain('latest:')
})

test('view: text output for package with dependencies shows deps count', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['@pnpm.e2e/pkg-with-1-dep@100.0.0']) as string
  const firstLine = result.split('\n')[0]
  expect(firstLine).toContain('deps: ')
  expect(firstLine).not.toContain('deps: none')
})

test('view: text output for deprecated package shows deprecation', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['@pnpm.e2e/deprecated@1.0.0']) as string
  expect(result).toMatch(/^DEPRECATED! - .+/m)
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

test('view: published info includes timestamp', async () => {
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0']) as string
  expect(result).toMatch(/published .* ago/)
})

test('view: published info includes publisher when maintainer data is available', async () => {
  // Note: is-negative package has maintainer data in the mock registry
  const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, ['is-negative@1.0.0']) as string
  expect(stripAnsi(result)).toMatch(/published .* ago by /)
})

test('view: uses package manifest name when no package name provided', async () => {
  const cwd = process.cwd()
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'view-test-'))
  const pkgJsonPath = path.join(tmpDir, 'package.json')

  try {
    fs.writeFileSync(pkgJsonPath, JSON.stringify({ name: 'is-negative' }))
    process.chdir(tmpDir)

    const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, [])
    expect(typeof result).toBe('string')
    expect(result).toContain('is-negative')
  } finally {
    process.chdir(cwd)
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('view: searches upward for package manifest in nested directory', async () => {
  const cwd = process.cwd()
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'view-test-'))
  const nestedDir = path.join(tmpDir, 'a', 'b')
  const pkgJsonPath = path.join(tmpDir, 'package.json')

  try {
    fs.mkdirSync(nestedDir, { recursive: true })
    fs.writeFileSync(pkgJsonPath, JSON.stringify({ name: 'is-negative' }))
    process.chdir(nestedDir)

    const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, [])
    expect(typeof result).toBe('string')
    expect(result).toContain('is-negative')
  } finally {
    process.chdir(cwd)
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('view: package.json without name field throws error', async () => {
  const cwd = process.cwd()
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'view-test-'))
  const pkgJsonPath = path.join(tmpDir, 'package.json')

  try {
    fs.writeFileSync(pkgJsonPath, JSON.stringify({ version: '1.0.0' }))
    process.chdir(tmpDir)

    await expect(
      view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_PACKAGE_JSON' })
  } finally {
    process.chdir(cwd)
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('view: uses package.yaml name when no package name provided', async () => {
  const cwd = process.cwd()
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'view-test-'))
  const pkgYamlPath = path.join(tmpDir, 'package.yaml')

  try {
    fs.writeFileSync(pkgYamlPath, 'name: is-negative\n')
    process.chdir(tmpDir)

    const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, [])
    expect(typeof result).toBe('string')
    expect(result).toContain('is-negative')
  } finally {
    process.chdir(cwd)
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('view: package.json with non-object JSON throws error', async () => {
  const cwd = process.cwd()
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'view-test-'))
  const pkgJsonPath = path.join(tmpDir, 'package.json')

  try {
    fs.writeFileSync(pkgJsonPath, 'null')
    process.chdir(tmpDir)

    await expect(
      view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, [])
    ).rejects.toMatchObject({ code: 'ERR_PNPM_INVALID_PACKAGE_JSON' })
  } finally {
    process.chdir(cwd)
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
})

test('view: resolves package.json from opts.dir when cwd differs', async () => {
  const cwd = process.cwd()
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'view-test-'))
  const pkgJsonPath = path.join(tmpDir, 'package.json')
  const otherDir = fs.mkdtempSync(path.join(os.tmpdir(), 'view-test-other-'))

  try {
    fs.writeFileSync(pkgJsonPath, JSON.stringify({ name: 'is-negative' }))
    process.chdir(otherDir)

    const result = await view.handler({ ...VIEW_OPTIONS, dir: tmpDir } as unknown as Config & ConfigContext, [])
    expect(typeof result).toBe('string')
    expect(result).toContain('is-negative')
  } finally {
    process.chdir(cwd)
    fs.rmSync(tmpDir, { recursive: true, force: true })
    fs.rmSync(otherDir, { recursive: true, force: true })
  }
})

test('view: derives package name even when engines.pnpm is incompatible', async () => {
  const cwd = process.cwd()
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'view-test-'))
  const pkgJsonPath = path.join(tmpDir, 'package.json')

  try {
    fs.writeFileSync(pkgJsonPath, JSON.stringify({
      name: 'is-negative',
      engines: {
        pnpm: '999.0.0',
      },
    }))
    process.chdir(tmpDir)

    const result = await view.handler(VIEW_OPTIONS as unknown as Config & ConfigContext, [])
    expect(typeof result).toBe('string')
    expect(result).toContain('is-negative')
  } finally {
    process.chdir(cwd)
    fs.rmSync(tmpDir, { recursive: true, force: true })
  }
})
