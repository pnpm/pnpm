import fs from 'node:fs/promises'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { preparePackages, tempDir } from '@pnpm/prepare'
import { pathExists } from 'path-exists'
import { readYamlFile } from 'read-yaml-file'
import { writeYamlFile } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

test('add --lockfile-only through symlinked node_modules does not mutate the target checkout', async () => {
  const testDir = tempDir()
  const primaryDir = path.join(testDir, 'primary')
  const worktreeDir = path.join(testDir, 'worktree')
  preparePackages([
    {
      location: 'primary',
      package: {
        name: 'root',
        private: true,
        dependencies: { 'is-odd': '3.0.1' },
      },
    },
    {
      location: 'primary/pkgs/a',
      package: {
        name: 'a',
        private: true,
        dependencies: { 'is-even': '1.0.0' },
      },
    },
    {
      location: 'primary/pkgs/b',
      package: {
        name: 'b',
        private: true,
        dependencies: { 'is-number': '7.0.0' },
      },
    },
  ], { tempDir: path.join(testDir, 'project') })
  await writeYamlFile(path.join(primaryDir, 'pnpm-workspace.yaml'), { packages: ['pkgs/*'] })
  execPnpmSync(['install'], { cwd: primaryDir, expectSuccess: true })

  const relativeDirs = ['', 'pkgs/a', 'pkgs/b']
  await Promise.all(relativeDirs.map(async relativeDir => {
    const targetDir = path.join(worktreeDir, relativeDir)
    await fs.mkdir(targetDir, { recursive: true })
    await fs.copyFile(path.join(primaryDir, relativeDir, 'package.json'), path.join(targetDir, 'package.json'))
  }))
  await Promise.all([
    fs.copyFile(path.join(primaryDir, 'pnpm-workspace.yaml'), path.join(worktreeDir, 'pnpm-workspace.yaml')),
    fs.copyFile(path.join(primaryDir, WANTED_LOCKFILE), path.join(worktreeDir, WANTED_LOCKFILE)),
  ])

  await Promise.all(relativeDirs.map(relativeDir =>
    fs.symlink(
      path.join(primaryDir, relativeDir, 'node_modules'),
      path.join(worktreeDir, relativeDir, 'node_modules'),
      process.platform === 'win32' ? 'junction' : 'dir'
    )
  ))

  const dependencyLinks = [
    path.join(primaryDir, 'node_modules/is-odd'),
    path.join(primaryDir, 'pkgs/a/node_modules/is-even'),
    path.join(primaryDir, 'pkgs/b/node_modules/is-number'),
  ]
  const modulesManifestPath = path.join(primaryDir, 'node_modules/.modules.yaml')
  const [linkTargetsBefore, modulesManifestBefore] = await Promise.all([
    Promise.all(dependencyLinks.map(link => fs.readlink(link))),
    fs.readFile(modulesManifestPath, 'utf8'),
  ])

  execPnpmSync(['add', 'left-pad', '--lockfile-only'], {
    cwd: path.join(worktreeDir, 'pkgs/a'),
    expectSuccess: true,
  })

  const [linkTargetsAfter, modulesManifestAfter, leftPadExists, packageJson, lockfile] = await Promise.all([
    Promise.all(dependencyLinks.map(link => fs.readlink(link))),
    fs.readFile(modulesManifestPath, 'utf8'),
    pathExists(path.join(primaryDir, 'pkgs/a/node_modules/left-pad')),
    fs.readFile(path.join(worktreeDir, 'pkgs/a/package.json'), 'utf8').then(JSON.parse),
    readYamlFile(path.join(worktreeDir, WANTED_LOCKFILE)) as Promise<{
      importers: Record<string, { dependencies?: Record<string, unknown> }>
    }>,
  ])
  expect(linkTargetsAfter).toStrictEqual(linkTargetsBefore)
  expect(modulesManifestAfter).toBe(modulesManifestBefore)
  expect(leftPadExists).toBe(false)
  expect(packageJson)
    .toHaveProperty('dependencies.left-pad')
  expect(lockfile.importers['pkgs/a'].dependencies).toHaveProperty('left-pad')
})
