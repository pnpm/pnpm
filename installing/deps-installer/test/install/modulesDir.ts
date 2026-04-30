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

// Regression test for https://github.com/pnpm/pnpm/issues/11403. The global
// add → approve-builds chain used to forward an absolute `modulesDir`
// (`<installDir>/node_modules`) into the install layer. `path.join` does not
// collapse an embedded absolute path, so the install layer would later
// produce a doubled path like `<installDir>/<installDir>/node_modules/...`
// and crash with `ENOENT` when hoist tried to mkdir/symlink under it.
test('installing with an absolute modules directory does not double the lockfileDir prefix', async () => {
  const project = prepareEmpty()
  const absoluteModulesDir = path.resolve('node_modules')

  await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }, testDefaults({ modulesDir: absoluteModulesDir, hoistPattern: ['*'] }))

  project.has('@pnpm.e2e/pkg-with-1-dep')
  // Verify the hoisted dep landed in node_modules/.pnpm/node_modules — i.e.
  // hoist did not silently fail or scribble the doubled path.
  expect(fs.existsSync(path.join(absoluteModulesDir, '.pnpm', 'node_modules', '@pnpm.e2e', 'dep-of-pkg-with-1-dep'))).toBe(true)

  // A frozen reinstall exercises the headless install path that originally
  // crashed with the doubled-prefix `ENOENT` while symlinking hoisted packages.
  rimrafSync(absoluteModulesDir)
  await install({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  }, testDefaults({ modulesDir: absoluteModulesDir, hoistPattern: ['*'], frozenLockfile: true }))

  project.has('@pnpm.e2e/pkg-with-1-dep')
  expect(fs.existsSync(path.join(absoluteModulesDir, '.pnpm', 'node_modules', '@pnpm.e2e', 'dep-of-pkg-with-1-dep'))).toBe(true)
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
