import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OUTDATED_OPTS } from './utils/index.js'

const originalModule = await import('@pnpm/deps.github-actions')
jest.unstable_mockModule('@pnpm/deps.github-actions', () => {
  return {
    ...originalModule,
    findOutdatedGitHubActions: jest.fn(async () => []),
  }
})

const { findOutdatedGitHubActions } = await import('@pnpm/deps.github-actions')
const { handler } = await import('../../src/outdated/outdated.js')

beforeEach(() => {
  jest.mocked(findOutdatedGitHubActions).mockClear()
})

test('outdated does not look at GitHub Actions by default', async () => {
  prepare({})
  writeWorkflow()

  await handler({ ...DEFAULT_OUTDATED_OPTS, dir: process.cwd() })

  expect(findOutdatedGitHubActions).not.toHaveBeenCalled()
})

test('outdated looks at GitHub Actions with --include-github-actions', async () => {
  prepare({})
  writeWorkflow()

  await handler({ ...DEFAULT_OUTDATED_OPTS, dir: process.cwd(), includeGithubActions: true })

  expect(findOutdatedGitHubActions).toHaveBeenCalledWith(expect.objectContaining({ dir: process.cwd() }))
})

test('outdated looks at GitHub Actions when update.githubActions is true', async () => {
  prepare({})
  writeWorkflow()

  await handler({ ...DEFAULT_OUTDATED_OPTS, dir: process.cwd(), updateConfig: { githubActions: true } })

  expect(findOutdatedGitHubActions).toHaveBeenCalledWith(expect.objectContaining({ dir: process.cwd() }))
})

test('recursive outdated does not look at GitHub Actions by default', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
  ])
  writeWorkflow()
  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  const opts = {
    ...DEFAULT_OUTDATED_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  }

  await handler(opts)

  expect(findOutdatedGitHubActions).not.toHaveBeenCalled()

  await handler({ ...opts, includeGithubActions: true })

  expect(findOutdatedGitHubActions).toHaveBeenCalledWith(expect.objectContaining({ dir: process.cwd() }))
})

function writeWorkflow (): void {
  fs.mkdirSync(path.join('.github', 'workflows'), { recursive: true })
  fs.writeFileSync(path.join('.github', 'workflows', 'ci.yml'), `jobs:
  test:
    steps:
      - uses: actions/checkout@v4.1.0
`)
}
