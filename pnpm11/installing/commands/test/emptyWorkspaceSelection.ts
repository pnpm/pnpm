import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { install } from '@pnpm/installing.commands'
import { preparePackages } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'

import { DEFAULT_OPTS } from './utils/index.js'

test.each([true, false])('an empty workspace install does not record installation proof (shared lockfile: %s)', async (sharedWorkspaceLockfile) => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0' } },
    { name: 'child', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } },
  ])
  const { allProjects, allProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    allProjectsGraph,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph: {},
    sharedWorkspaceLockfile,
    workspaceDir: process.cwd(),
  })

  expect(fs.existsSync(path.resolve('node_modules/.pnpm-workspace-state-v1.json'))).toBe(false)
})
