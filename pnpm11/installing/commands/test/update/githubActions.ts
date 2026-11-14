import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { DEFAULT_OPTS } from '../utils/index.js'

const originalModule = await import('@pnpm/deps.github-actions')
jest.unstable_mockModule('@pnpm/deps.github-actions', () => {
  return {
    ...originalModule,
    findOutdatedGitHubActions: jest.fn(async () => []),
    updateGitHubActions: jest.fn(async () => []),
  }
})

const { findOutdatedGitHubActions, updateGitHubActions } = await import('@pnpm/deps.github-actions')
const { handler } = await import('../../src/update/index.js')
const { createGitHubActionsMatcher } = await import('../../src/update/update.js')

beforeEach(() => {
  jest.mocked(findOutdatedGitHubActions).mockClear()
  jest.mocked(updateGitHubActions).mockClear()
  prepare({})
  fs.mkdirSync(path.join('.github', 'workflows'), { recursive: true })
  fs.writeFileSync(path.join('.github', 'workflows', 'ci.yml'), `jobs:
  test:
    steps:
      - uses: actions/checkout@v4.1.0
`)
})

test('update --interactive does not look for GitHub Actions updates by default', async () => {
  await handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    interactive: true,
  })

  expect(findOutdatedGitHubActions).not.toHaveBeenCalled()
})

test('update --interactive looks for GitHub Actions updates with --include-github-actions', async () => {
  await handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    includeGithubActions: true,
    interactive: true,
  })

  expect(findOutdatedGitHubActions).toHaveBeenCalledWith(expect.objectContaining({ dir: process.cwd() }))
})

test('update --interactive looks for GitHub Actions updates when update.githubActions is true', async () => {
  await handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    interactive: true,
    updateConfig: { githubActions: true },
  })

  expect(findOutdatedGitHubActions).toHaveBeenCalledWith(expect.objectContaining({ dir: process.cwd() }))
})

test('createGitHubActionsMatcher supports repo, subpath, negated, and mixed selectors', () => {
  const repoMatch = createGitHubActionsMatcher(['actions/cache'])!
  expect(repoMatch('actions/cache')).toBe(true)
  expect(repoMatch('actions/cache/save')).toBe(true)
  expect(repoMatch('actions/checkout')).toBe(false)

  const negatedSubpath = createGitHubActionsMatcher(['!actions/cache/save'])!
  expect(negatedSubpath('actions/cache/save')).toBe(false)
  expect(negatedSubpath('actions/cache/restore')).toBe(true)
  expect(negatedSubpath('actions/cache')).toBe(true)
  expect(negatedSubpath('actions/checkout')).toBe(true)

  const negatedRepo = createGitHubActionsMatcher(['!actions/cache'])!
  expect(negatedRepo('actions/cache')).toBe(false)
  expect(negatedRepo('actions/cache/save')).toBe(false)
  expect(negatedRepo('actions/checkout')).toBe(true)

  const mixed = createGitHubActionsMatcher(['actions/cache', '!actions/cache/save'])!
  expect(mixed('actions/cache')).toBe(true)
  expect(mixed('actions/cache/restore')).toBe(true)
  expect(mixed('actions/cache/save')).toBe(false)
  expect(mixed('actions/checkout')).toBe(false)
})

test('update with a negated action selector still updates npm dependencies', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '100.0.0',
    },
  })
  await handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['@pnpm.e2e/dep-of-pkg-with-1-dep@100.0.0'])

  const manifestPath = path.join(process.cwd(), 'package.json')
  fs.writeFileSync(manifestPath, JSON.stringify({
    name: 'project',
    version: '0.0.0',
    dependencies: {
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '^100.0.0',
    },
  }, null, 2))

  await handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    includeGithubActions: true,
  }, ['!actions/checkout'])

  expect(updateGitHubActions).toHaveBeenCalledTimes(1)
  const lockfile = fs.readFileSync(path.join(process.cwd(), 'pnpm-lock.yaml'), 'utf8')
  expect(lockfile).toContain('100.1.0')
})
