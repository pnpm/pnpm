import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { add, install } from '@pnpm/installing.commands'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { ProjectManifest } from '@pnpm/types'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import delay from 'delay'
import { loadJsonFileSync } from 'load-json-file'

import { DEFAULT_OPTS } from './utils/index.js'

test.each([
  ['storeDir', { storeDir: '../other-store' }, 'ERR_PNPM_UNEXPECTED_STORE', 'storeDir', 'other-store'],
  ['virtualStoreDir', { virtualStoreDir: 'node_modules/.vs' }, 'ERR_PNPM_UNEXPECTED_VIRTUAL_STORE', 'virtualStoreDir', '.vs'],
])('install recreates node_modules after a %s change that add refuses', async (_setting, changedOpts, code, modulesField, expected) => {
  prepareEmpty()
  const opts = {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    rootProjectManifestDir: process.cwd(),
    confirmModulesPurge: false,
    optimisticRepeatInstall: true,
    pnpmfile: [],
  }
  await add.handler(opts, ['is-positive@1.0.0'])
  const installOpts = { ...opts, rootProjectManifest: loadJsonFileSync<ProjectManifest>(path.resolve('package.json')) }
  const modulesYamlMtime = fs.statSync('node_modules/.modules.yaml').mtimeMs
  await delay(50)
  await install.handler(installOpts)
  expect(fs.statSync('node_modules/.modules.yaml').mtimeMs).toBe(modulesYamlMtime)

  await expect(add.handler({ ...opts, ...changedOpts }, ['is-negative@1.0.0'])).rejects.toMatchObject({ code })

  await install.handler({ ...installOpts, ...changedOpts })

  const modules = loadJsonFileSync<Record<string, string>>(path.resolve('node_modules/.modules.yaml'))
  expect(modules[modulesField]).toContain(expected)
  expect(fs.existsSync('node_modules/is-positive')).toBe(true)
})

test('a single-project install records the configured storeDir, not the resolved store', async () => {
  prepareEmpty()
  await add.handler({ ...DEFAULT_OPTS, dir: process.cwd() }, ['is-positive@1.0.0'])

  expect(readRecordedStoreDir()).toBe(DEFAULT_OPTS.storeDir)
})

test('a recursive install records the configured storeDir, not the resolved store', async () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0', dependencies: { 'is-positive': '1.0.0' } },
  ])
  const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    workspaceDir: process.cwd(),
  })

  expect(readRecordedStoreDir()).toBe(DEFAULT_OPTS.storeDir)
})

function readRecordedStoreDir (): string | undefined {
  return loadJsonFileSync<{ settings: { storeDir?: string } }>(path.resolve('node_modules/.pnpm-workspace-state-v1.json')).settings.storeDir
}
