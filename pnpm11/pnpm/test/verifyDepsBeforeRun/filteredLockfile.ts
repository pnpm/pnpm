import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { readCurrentLockfile } from '@pnpm/lockfile.fs'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

test('a filtered frozen install adds a new workspace project to the current lockfile', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', private: true } },
    { location: 'packages/a', package: { name: 'a', dependencies: { '@pnpm.e2e/foo': '100.0.0' } } },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })
  await execPnpm(['install'])
  fs.mkdirSync('packages/b')
  fs.writeFileSync('packages/b/package.json', JSON.stringify({
    name: 'b',
    dependencies: { '@pnpm.e2e/bar': '100.0.0' },
  }))
  await execPnpm(['install', '--lockfile-only'])
  await execPnpm(['install', '--filter=b', '--frozen-lockfile'])

  const current = await readCurrentLockfile('node_modules/.pnpm', { ignoreIncompatible: false })
  expect(current?.importers).toHaveProperty(['packages/b', 'dependencies', '@pnpm.e2e/bar'], '100.0.0')
  expect(Object.keys(current!.packages!).sort()).toStrictEqual([
    '@pnpm.e2e/bar@100.0.0',
    '@pnpm.e2e/foo@100.0.0',
  ])
  expect(fs.existsSync('packages/b/node_modules/@pnpm.e2e/bar')).toBe(true)
})

test('filtered installs retain unselected packages and drop obsolete selected packages', async () => {
  const projects = preparePackages([
    { location: '.', package: { name: 'root', private: true } },
    { location: 'packages/a', package: { name: 'a', private: true, dependencies: { '@pnpm.e2e/foo': '100.0.0' } } },
    { location: 'packages/b', package: { name: 'b', private: true, dependencies: { '@pnpm.e2e/pkg-with-1-dep': '100.0.0' } } },
    { location: 'packages/c', package: { name: 'c', private: true, dependencies: { a: 'workspace:*', '@pnpm.e2e/bar': '100.0.0' } } },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })
  await execPnpm(['install'])
  projects.a.writePackageJson({ name: 'a', private: true, dependencies: { '@pnpm.e2e/bar': '100.0.0' } })
  projects.b.writePackageJson({ name: 'b', private: true, dependencies: { '@pnpm.e2e/foo': '100.0.0' } })
  await execPnpm(['install', '--lockfile-only'])

  execPnpmSync(['--config.verify-deps-before-run=install', '--filter=a', 'exec', 'node', '-e', '0'], { expectSuccess: true })
  const afterFirstInstall = await readCurrentLockfile('node_modules/.pnpm', { ignoreIncompatible: false })
  expect(afterFirstInstall?.packages).toHaveProperty(['@pnpm.e2e/pkg-with-1-dep@100.0.0'])
  expect(afterFirstInstall?.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0'])

  execPnpmSync(['--config.verify-deps-before-run=install', '--filter=b', 'exec', 'node', '-e', '0'], { expectSuccess: true })
  const afterSecondInstall = await readCurrentLockfile('node_modules/.pnpm', { ignoreIncompatible: false })
  expect(Object.keys(afterSecondInstall!.packages!).sort()).toStrictEqual([
    '@pnpm.e2e/bar@100.0.0',
    '@pnpm.e2e/foo@100.0.0',
  ])
  for (const projectName of ['a', 'b', 'c']) {
    execPnpmSync(['--config.verify-deps-before-run=error', `--filter=${projectName}`, 'exec', 'node', '-e', '0'], { expectSuccess: true })
  }
  await execPnpm(['install'])
  const repeated = fs.readFileSync('node_modules/.pnpm/lock.yaml', 'utf8')
  await execPnpm(['install', '--config.optimistic-repeat-install=false'])
  expect(fs.readFileSync('node_modules/.pnpm/lock.yaml', 'utf8')).toBe(repeated)
})
