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
  }
})

const { findOutdatedGitHubActions } = await import('@pnpm/deps.github-actions')
const { handler } = await import('../../src/update/index.js')

beforeEach(() => {
  jest.mocked(findOutdatedGitHubActions).mockClear()
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
