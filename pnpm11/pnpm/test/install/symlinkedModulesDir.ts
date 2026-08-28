import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { preparePackages, tempDir } from '@pnpm/prepare'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

test('add --lockfile-only through symlinked node_modules does not mutate the target checkout', () => {
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
  writeYamlFileSync(path.join(primaryDir, 'pnpm-workspace.yaml'), { packages: ['pkgs/*'] })
  execPnpmSync(['install'], { cwd: primaryDir, expectSuccess: true })

  for (const relativeDir of ['', 'pkgs/a', 'pkgs/b']) {
    const targetDir = path.join(worktreeDir, relativeDir)
    fs.mkdirSync(targetDir, { recursive: true })
    fs.copyFileSync(path.join(primaryDir, relativeDir, 'package.json'), path.join(targetDir, 'package.json'))
  }
  fs.copyFileSync(path.join(primaryDir, 'pnpm-workspace.yaml'), path.join(worktreeDir, 'pnpm-workspace.yaml'))
  fs.copyFileSync(path.join(primaryDir, WANTED_LOCKFILE), path.join(worktreeDir, WANTED_LOCKFILE))

  for (const relativeDir of ['', 'pkgs/a', 'pkgs/b']) {
    fs.symlinkSync(
      path.join(primaryDir, relativeDir, 'node_modules'),
      path.join(worktreeDir, relativeDir, 'node_modules'),
      process.platform === 'win32' ? 'junction' : 'dir'
    )
  }

  const dependencyLinks = [
    path.join(primaryDir, 'node_modules/is-odd'),
    path.join(primaryDir, 'pkgs/a/node_modules/is-even'),
    path.join(primaryDir, 'pkgs/b/node_modules/is-number'),
  ]
  const linkTargetsBefore = dependencyLinks.map(link => fs.readlinkSync(link))
  const modulesManifestPath = path.join(primaryDir, 'node_modules/.modules.yaml')
  const modulesManifestBefore = fs.readFileSync(modulesManifestPath, 'utf8')

  execPnpmSync(['add', 'left-pad', '--lockfile-only'], {
    cwd: path.join(worktreeDir, 'pkgs/a'),
    expectSuccess: true,
  })

  expect(dependencyLinks.map(link => fs.readlinkSync(link))).toStrictEqual(linkTargetsBefore)
  expect(fs.readFileSync(modulesManifestPath, 'utf8')).toBe(modulesManifestBefore)
  expect(fs.existsSync(path.join(primaryDir, 'pkgs/a/node_modules/left-pad'))).toBe(false)
  expect(JSON.parse(fs.readFileSync(path.join(worktreeDir, 'pkgs/a/package.json'), 'utf8')))
    .toHaveProperty('dependencies.left-pad')
  const lockfile = readYamlFileSync(path.join(worktreeDir, WANTED_LOCKFILE)) as {
    importers: Record<string, { dependencies?: Record<string, unknown> }>
  }
  expect(lockfile.importers['pkgs/a'].dependencies).toHaveProperty('left-pad')
})
