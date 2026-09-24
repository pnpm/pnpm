import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import type { ProjectManifest } from '@pnpm/types'
import { loadWorkspaceState } from '@pnpm/workspace.state'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync, spawnPnpm, waitForPnpmExit } from '../utils/index.js'

const CONFIG = [
  '--config.verify-deps-before-run=install',
  '--reporter=append-only',
] as const

test('verify-deps-before-run=install reuses the same flags as specified by the workspace state (#9109)', async () => {
  const manifest: ProjectManifest = {
    name: 'root',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/bar': '100.0.0',
    },
    scripts: {
      start: 'echo hello from script',
      postinstall: 'echo install was executed',
    },
  }

  const project = prepare(manifest)

  await execPnpm([...CONFIG, 'install'])

  // --production
  {
    fs.rmSync('node_modules', { recursive: true })
    await execPnpm([...CONFIG, 'install', '--production', '--frozen-lockfile'])
    project.has('@pnpm.e2e/foo')
    project.hasNot('@pnpm.e2e/bar')
    expect(loadWorkspaceState(process.cwd())).toMatchObject({
      settings: {
        dev: false,
        optional: true,
        production: true,
      },
    })

    project.writePackageJson({
      ...manifest,
      dependencies: {
        ...manifest.dependencies,
        '@pnpm.e2e/foo': '100.1.0', // different from the initial manifest
      },
    })

    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('install was executed')
    expect(stdout.toString()).toContain('hello from script')
    project.has('@pnpm.e2e/foo')
    project.hasNot('@pnpm.e2e/bar')
    expect(loadWorkspaceState(process.cwd())).toMatchObject({
      settings: {
        dev: false,
        optional: true,
        production: true,
      },
    })
  }

  // --dev
  {
    fs.rmSync('node_modules', { recursive: true })
    await execPnpm([...CONFIG, 'install', '--dev', '--frozen-lockfile'])
    project.hasNot('@pnpm.e2e/foo')
    project.has('@pnpm.e2e/bar')
    expect(loadWorkspaceState(process.cwd())).toMatchObject({
      settings: {
        dev: true,
        optional: false,
        production: false,
      },
    })

    project.writePackageJson({
      ...manifest,
      dependencies: {
        ...manifest.dependencies,
        '@pnpm.e2e/foo': '100.0.0', // different from the manifest created by --production
      },
    })

    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('install was executed')
    expect(stdout.toString()).toContain('hello from script')
    project.hasNot('@pnpm.e2e/foo')
    project.has('@pnpm.e2e/bar')
    expect(loadWorkspaceState(process.cwd())).toMatchObject({
      settings: {
        dev: true,
        optional: false,
        production: false,
      },
    })
  }

  // neither --dev nor --production
  {
    fs.rmSync('node_modules', { recursive: true })
    await execPnpm([...CONFIG, 'install', '--frozen-lockfile'])
    project.has('@pnpm.e2e/foo')
    project.has('@pnpm.e2e/bar')
    expect(loadWorkspaceState(process.cwd())).toMatchObject({
      settings: {
        dev: true,
        optional: true,
        production: true,
      },
    })

    project.writePackageJson({
      ...manifest,
      dependencies: {
        ...manifest.dependencies,
        '@pnpm.e2e/foo': '100.1.0', // different from the manifest created by --dev
      },
    })

    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('install was executed')
    expect(stdout.toString()).toContain('hello from script')
    project.has('@pnpm.e2e/foo')
    project.has('@pnpm.e2e/bar')
    expect(loadWorkspaceState(process.cwd())).toMatchObject({
      settings: {
        dev: true,
        optional: true,
        production: true,
      },
    })
  }
})

test('concurrent runs on a stale workspace start one install (#14551)', async () => {
  const root: ProjectManifest = {
    name: 'root',
    private: true,
    scripts: {
      hello: 'echo hello from script',
      // Keep the install running until every run has checked the stale tree.
      postinstall: 'node -e "require(\'fs\').appendFileSync(\'installs.log\', \'installed\\n\'); setTimeout(() => {}, 2000)"',
    },
  }
  const project = preparePackages([
    { location: '.', package: root },
    { name: 'project', version: '1.0.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm([...CONFIG, 'install'])
  fs.rmSync('installs.log')

  project.root.writePackageJson({ ...root, dependencies: { project: 'workspace:*' } })
  const runs = Array.from({ length: 4 }, () => spawnPnpm([...CONFIG, 'hello']))
  const results = await Promise.all(runs.map(async (run) => waitForPnpmExit(run)))

  for (const { status, stdout, stderr } of results) {
    expect({ status, output: stdout.toString() + stderr.toString() }).toMatchObject({ status: 0 })
  }
  expect(fs.readFileSync('installs.log', 'utf8').split('\n').filter(Boolean)).toHaveLength(1)
  project.root.has('project')
})
