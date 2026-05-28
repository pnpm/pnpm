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

const { repo } = await import('@pnpm/deps.inspection.commands')

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`

const BASE_OPTIONS = {
  registries: { default: REGISTRY_URL },
} as unknown as Config & ConfigContext & { dir: string }

test('repo: command should be available', () => {
  expect(repo.handler).toBeDefined()
  expect(repo.help).toBeDefined()
  expect(repo.commandNames).toEqual(['repo'])
})

test('repo: opens repository URL from local manifest', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: 'https://github.com/test/pkg',
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg')
})

test('repo: opens repository object URL from local manifest', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'https://github.com/test/pkg' },
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg')
})

test('repo: normalizes git+https repository URL with .git suffix', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+https://github.com/test/pkg.git' },
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg')
})

test('repo: trims trailing slash from repository URL', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: 'https://github.com/test/pkg/',
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg')
})

test('repo: resolves repository shorthand (owner/repo)', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: 'test/pkg',
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg')
})

test('repo: resolves github: shorthand', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'github:test/pkg' },
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg')
})

test('repo: resolves git+ssh:// repository URL', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+ssh://git@github.com/test/pkg.git' },
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg')
})

test('repo: resolves gitlab: shorthand', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: 'gitlab:test/pkg',
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://gitlab.com/test/pkg')
})

test('repo: handles repository URL ending with .git/ (trailing slash after .git)', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+https://github.com/test/pkg.git/' },
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg')
})

test('repo: uses fragment as branch in repository URL', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+https://github.com/test/pkg.git#main' },
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/tree/main')
})

test('repo: appends directory for monorepo packages', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'https://github.com/test/pkg', directory: 'packages/foo' },
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/tree/master/packages/foo')
})

test('repo: resolves shorthand with directory for monorepo packages', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'test/pkg', directory: 'packages/bar' },
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://github.com/test/pkg/tree/master/packages/bar')
})

test('repo: falls back to URL parsing for self-hosted git servers', async () => {
  mockOpen.mockClear()
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
    repository: { url: 'git+https://git.example.com/test/pkg.git' },
  }))
  await repo.handler({ ...BASE_OPTIONS, dir }, [])
  expect(mockOpen).toHaveBeenCalledWith('https://git.example.com/test/pkg')
})

test('repo: throws when no repository URL is defined', async () => {
  const dir = tempDir()
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: 'test-pkg',
  }))
  await expect(
    repo.handler({ ...BASE_OPTIONS, dir }, [])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_NO_REPO_URL' })
})

test('repo: throws when no package.json exists', async () => {
  const dir = tempDir()
  await expect(
    repo.handler({ ...BASE_OPTIONS, dir }, [])
  ).rejects.toMatchObject({ code: 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND' })
})

test('repo: looks up package on registry by name', async () => {
  mockOpen.mockClear()
  await repo.handler(BASE_OPTIONS, ['is-negative'])
  expect(mockOpen).toHaveBeenCalledTimes(1)
  const calledUrl = mockOpen.mock.calls[0][0]
  expect(typeof calledUrl).toBe('string')
  expect((calledUrl as string).startsWith('http')).toBe(true)
})
