import fs from 'node:fs'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from './utils/index.js'

const warn = jest.fn()
const info = jest.fn()
const debug = jest.fn()
const original = await import('@pnpm/logger')
jest.unstable_mockModule('@pnpm/logger', () => ({
  ...original,
  logger: Object.assign(() => ({ warn, info, debug }), { warn, info, debug }),
}))
const { install } = await import('@pnpm/installing.commands')

afterEach(() => {
  jest.mocked(warn).mockRestore()
})

async function installWorkspace (selectors: Array<{ namePattern: string }> = []): Promise<void> {
  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), selectors)
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })
}

test('warns that the pnpm-workspace.yaml of a workspace project does not apply', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0' },
    { name: 'project-2', version: '1.0.0' },
  ])
  fs.writeFileSync(path.join('project-2', 'pnpm-workspace.yaml'), 'overrides:\n  foo: 1.0.0\n')

  await installWorkspace()

  expect(warn).toHaveBeenCalledTimes(1)
  expect(warn).toHaveBeenCalledWith({
    message: 'The settings in project-2/pnpm-workspace.yaml do not apply, because project-2 is a project of this workspace. ' +
      'pnpm reads settings only from the pnpm-workspace.yaml at the workspace root. ' +
      'Move the settings there, or add "!project-2" to the root\'s "packages" to keep that project a separate workspace.',
    prefix: process.cwd(),
  })
})

test('does not warn about the pnpm-workspace.yaml of the workspace root', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0' },
    { name: 'project-2', version: '1.0.0' },
  ])
  fs.writeFileSync('pnpm-workspace.yaml', 'packages:\n  - "*"\n')
  fs.writeFileSync('package.json', JSON.stringify({ name: 'root', version: '1.0.0' }))

  await installWorkspace()

  expect(warn).toHaveBeenCalledTimes(0)
})

test('does not warn when the install leaves out the project with its own pnpm-workspace.yaml', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0' },
    { name: 'project-2', version: '1.0.0' },
  ])
  fs.writeFileSync(path.join('project-2', 'pnpm-workspace.yaml'), 'overrides:\n  foo: 1.0.0\n')

  await installWorkspace([{ namePattern: 'project-1' }])

  expect(warn).toHaveBeenCalledTimes(0)
})
