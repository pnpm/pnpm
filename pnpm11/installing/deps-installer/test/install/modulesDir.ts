import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import {
  install,
  type MutatedProject,
  mutateModules,
} from '@pnpm/installing.deps-installer'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'
import { rimrafSync } from '@zkochan/rimraf'
import { safeExeca as execa } from 'execa'
import { writeJsonFileSync } from 'write-json-file'

import { testDefaults } from '../utils/index.js'

test('installing to a custom modules directory', async () => {
  const project = prepareEmpty()

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({ modulesDir: 'pnpm_modules' }))

  project.has('is-positive', 'pnpm_modules')

  rimrafSync('pnpm_modules')
  project.hasNot('is-positive', 'pnpm_modules')

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, testDefaults({ frozenLockfile: true, modulesDir: 'pnpm_modules' }))

  project.has('is-positive', 'pnpm_modules')
})

test('using different custom modules directory for every project', async () => {
  const projects = preparePackages([
    {
      location: 'project-1',
      package: {
        name: 'project-1',

        dependencies: { 'is-positive': '1.0.0' },
      },
    },
    {
      location: 'project-2',
      package: {
        name: 'project-2',

        dependencies: { 'is-positive': '1.0.0' },
      },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      modulesDir: 'modules_1',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',

        dependencies: {
          'is-positive': '1.0.0',
        },
      },
      modulesDir: 'modules_2',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({ allProjects }))

  projects['project-1'].has('is-positive', 'modules_1')
  projects['project-2'].has('is-positive', 'modules_2')
})

test('bins in a custom modules directory load plugins installed in that directory', async () => {
  prepareEmpty()
  writeTool()
  writePlugin()
  const manifest = {
    dependencies: {
      'is-positive': '3.1.0',
      plugin: 'file:plugin',
      tool: 'file:tool',
    },
  }

  await install(manifest, testDefaults({ modulesDir: 'vendor', hoistPattern: '*' }))
  expect(await runTool('vendor')).toStrictEqual({ plugin: 'plugin loaded', isPositive: '1.0.0' })

  rimrafSync('vendor')
  await install(manifest, testDefaults({ frozenLockfile: true, modulesDir: 'vendor', hoistPattern: '*' }))
  expect(await runTool('vendor')).toStrictEqual({ plugin: 'plugin loaded', isPositive: '1.0.0' })
})

test('bins in a custom modules directory are relinked when extendNodePath changes', async () => {
  prepareEmpty()
  writeTool()
  writePlugin()
  const manifest = {
    dependencies: {
      plugin: 'file:plugin',
      tool: 'file:tool',
    },
  }
  // With scripts, the lifecycle step relinks every project bin anyway.
  const opts = { modulesDir: 'vendor', ignoreScripts: true }

  await install(manifest, testDefaults({ ...opts, extendNodePath: false }))
  await expect(runTool('vendor')).rejects.toThrow("Cannot find module 'plugin'")

  await install(manifest, testDefaults(opts))
  expect(await runTool('vendor')).toMatchObject({ plugin: 'plugin loaded' })

  await install(manifest, testDefaults({ ...opts, extendNodePath: false }))
  await expect(runTool('vendor')).rejects.toThrow("Cannot find module 'plugin'")
})

test('bins of every project load plugins only from that project\'s modules directory', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])
  writeTool()
  writePlugin()

  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',
        dependencies: {
          tool: 'file:../tool',
        },
      },
      modulesDir: 'modules_1',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',
        dependencies: {
          plugin: 'file:../plugin',
          tool: 'file:../tool',
        },
      },
      modulesDir: 'modules_2',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  await mutateModules(allProjects.map(({ rootDir }) => ({ mutation: 'install', rootDir })), testDefaults({ allProjects }))

  await expect(runTool('modules_1', 'project-1')).rejects.toThrow("Cannot find module 'plugin'")
  expect(await runTool('modules_2', 'project-2')).toMatchObject({ plugin: 'plugin loaded' })
})

// Loads plugins from the working directory, the way ESLint and similar tools do.
function writeTool (): void {
  writeJsonFileSync('tool/package.json', {
    name: 'tool',
    version: '1.0.0',
    bin: 'bin.js',
    dependencies: {
      'is-positive': '1.0.0',
    },
  })
  fs.writeFileSync('tool/bin.js', `#!/usr/bin/env node
const { createRequire } = require('node:module')
const path = require('node:path')
const requireFromProject = createRequire(path.join(process.cwd(), 'package.json'))
console.log(JSON.stringify({
  plugin: requireFromProject('plugin'),
  isPositive: requireFromProject('is-positive/package.json').version,
}))
`)
}

function writePlugin (): void {
  writeJsonFileSync('plugin/package.json', { name: 'plugin', version: '1.0.0' })
  fs.writeFileSync('plugin/index.js', "module.exports = 'plugin loaded'\n")
}

async function runTool (modulesDir: string, cwd = process.cwd()): Promise<unknown> {
  const { stdout } = await execa(path.resolve(cwd, modulesDir, '.bin/tool'), [], { cwd: path.resolve(cwd) })
  return JSON.parse(stdout)
}
