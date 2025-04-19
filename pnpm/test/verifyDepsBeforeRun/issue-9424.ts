import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { type WorkspaceState, loadWorkspaceState } from '@pnpm/workspace.state'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync } from '../utils'

test('hoisted node linker and node_modules not exist (#9424)', async () => {
  const config = [
    '--config.verify-deps-before-run=error',
    '--config.node-linker=hoisted',
  ] as const

  type PackageName = 'has-deps' | 'has-no-deps'
  const manifests: Record<PackageName, ProjectManifest> = {
    'has-deps': {
      name: 'has-deps',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
      scripts: {
        start: 'echo hello from has-deps',
      },
    },
    'has-no-deps': {
      name: 'has-no-deps',
      private: true,
      scripts: {
        start: 'echo hello from has-no-deps',
      },
    },
  }

  preparePackages([manifests['has-deps'], manifests['has-no-deps']])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  // attempting to execute a script recursively without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--recursive', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...config, 'install'])

  // pnpm install should create a packages list cache
  expect(loadWorkspaceState(process.cwd())).toMatchObject({
    lastValidatedTimestamp: expect.any(Number),
    pnpmfileExists: false,
    filteredInstall: false,
    projects: {
      [path.resolve('has-deps')]: { name: 'has-deps', version: '0.0.0' },
      [path.resolve('has-no-deps')]: { name: 'has-no-deps', version: '0.0.0' },
    },
    settings: {
      nodeLinker: 'hoisted',
    },
  } as Partial<WorkspaceState>)

  // pnpm install creates a node_modules at root, but none in the workspace members
  expect(fs.readdirSync(process.cwd())).toContain('node_modules')
  expect(fs.readdirSync(path.resolve('has-deps'))).not.toContain('node_modules')
  expect(fs.readdirSync(path.resolve('has-no-deps'))).not.toContain('node_modules')

  // should be able to execute a script recursively after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...config, '--recursive', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from has-deps')
    expect(stdout.toString()).toContain('hello from has-no-deps')
  }
})
