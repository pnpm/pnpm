import fs from 'fs'
import path from 'path'
import { preparePackages, tempDir } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import {
  type MutatedProject,
  mutateModules,
  install,
} from '@pnpm/core'
import { type ProjectRootDir } from '@pnpm/types'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test('jest CLI should print the right version when multiple instances of jest are used in a workspace', async () => {
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
        scripts: {
          postinstall: 'jest --version > output.json',
        },

        dependencies: {
          jest: '27.5.1',
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',
        scripts: {
          postinstall: 'jest --version > output.json',
        },

        dependencies: {
          jest: '24.9.0',
        },
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  await mutateModules(importers, testDefaults({
    allProjects,
    extendNodePath: true,
    fastUnpack: false,
    hoistPattern: '*',
  }))

  {
    const jestVersion = fs.readFileSync('project-1/output.json').toString()
    expect(jestVersion.trim()).toStrictEqual('27.5.1')
  }
  {
    const jestVersion = fs.readFileSync('project-2/output.json').toString()
    expect(jestVersion.trim()).toStrictEqual('24.9.0')
  }
})

test('drupal-js-build should find plugins inside the hidden node_modules directory', async () => {
  const tmp = tempDir()
  f.copy('tooling-that-needs-node-path', tmp)
  await install({
    dependencies: {
      'drupal-js-build': 'github:pnpm-e2e/drupal-js-build#f766801580f10543c24ba8bfa59046a776848097',
    },
    scripts: {
      prepare: 'drupal-js-build',
    },
  }, testDefaults({
    extendNodePath: true,
    fastUnpack: false,
    hoistPattern: '*',
  }))
  expect(fs.existsSync(path.join(tmp, 'index.js'))).toBeTruthy()
})
