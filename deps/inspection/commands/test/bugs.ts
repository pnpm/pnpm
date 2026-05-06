import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { tempDir } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'

const mockOpen = jest.fn()
jest.unstable_mockModule('open', () => ({
  default: mockOpen,
}))

const { bugs } = await import('@pnpm/deps.inspection.commands')

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const BASE_OPTIONS = {
  registries: { default: REGISTRY_URL },
} as unknown as Config & ConfigContext & { dir: string }

test('bugs: command should be available', () => {
  expect(bugs.handler).toBeDefined()
  expect(bugs.help).toBeDefined()
  expect(bugs.commandNames).toEqual(['bugs'])
})

test('bugs: opens bugs.url from local manifest', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    bugs: { url: 'https://github.com/test/pkg/issues' },
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: opens bugs string URL from local manifest', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    bugs: 'https://github.com/test/pkg/issues',
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: falls back to repository/issues URL when bugs is missing', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: 'https://github.com/test/pkg',
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: normalizes git+https repository URL with .git suffix', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+https://github.com/test/pkg.git' },
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: trims trailing slash from repository URL before appending /issues', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: 'https://github.com/test/pkg/',
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: resolves repository shorthand (owner/repo)', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: 'test/pkg',
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: resolves github: shorthand', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'github:test/pkg' },
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: resolves git+ssh:// repository URL', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+ssh://git@github.com/test/pkg.git' },
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: resolves gitlab: shorthand', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: 'gitlab:test/pkg',
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://gitlab.com/test/pkg/issues')
})

test('bugs: falls back to URL parsing for self-hosted git servers', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+https://git.example.com/test/pkg.git' },
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://git.example.com/test/pkg/issues')
})

test('bugs: handles repository URL ending with .git/ (trailing slash after .git)', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+https://github.com/test/pkg.git/' },
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: strips fragment/query from repository URL before appending /issues', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+https://github.com/test/pkg.git#main' },
  }))
  await bugs.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/issues')
})

test('bugs: throws when no bugs URL can be derived', async () => {
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
  }))
  await expect(
    bugs.handler({ ...BASE_OPTIONS, dir }, [])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_NO_BUGS_URL' })
})

test('bugs: throws when no package.json exists', async () => {
  const dir = tempDir()
  await expect(
    bugs.handler({ ...BASE_OPTIONS, dir }, [])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND' })
})

test('bugs: looks up package on registry by name', async () => {
  mockOpen.mockClear()
  await bugs.handler(BASE_OPTIONS, ['is-negative'])
  expect(mockOpen).toHaveBeenCalledTimes(1)
  const calledUrl = mockOpen.mock.calls[0][0]
  expect(typeof calledUrl).toBe('string')
  expect((calledUrl as string).startsWith('http')).toBe(true)
})
