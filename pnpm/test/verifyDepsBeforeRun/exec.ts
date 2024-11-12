import fs from 'fs'
import path from 'path'
import { prepare, preparePackages } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { loadWorkspaceState } from '@pnpm/workspace.state'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync } from '../utils'

const CONFIG = ['--config.verify-deps-before-run=true'] as const

test('single package workspace', async () => {
  const manifest: ProjectManifest = {
    name: 'root',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
    },
  }

  const project = prepare(manifest)

  const EXEC = ['exec', 'echo', 'hello from exec'] as const

  // attempting to execute a command without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND')
  }

  await execPnpm([...CONFIG, 'install'])

  // installing dependencies on a single package workspace should not create a packages list cache
  {
    const workspaceState = loadWorkspaceState(process.cwd())
    expect(workspaceState).toBeUndefined()
  }

  // should be able to execute a command after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from exec')
  }

  project.writePackageJson(manifest)

  // should be able to execute a command after the mtime of the manifest change but the content doesn't
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from exec')
  }

  project.writePackageJson({
    ...manifest,
    dependencies: {
      ...manifest.dependencies,
      '@pnpm.e2e/foo': '100.1.0',
    },
  })

  // attempting to execute a command with outdated dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
  }

  await execPnpm([...CONFIG, 'install'])

  // should be able to execute a command after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from exec')
  }

  project.writePackageJson({
    ...manifest,
    dependencies: {}, // delete all dependencies
  })

  // attempting to execute a command with redundant dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
  }

  await execPnpm([...CONFIG, 'install'])

  // should be able to execute a command without dependencies
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from exec')
  }

  // should set env.pnpm_run_skip_deps_check for the script
  await execPnpm([...CONFIG, 'exec', 'node', '--eval', 'assert.strictEqual(process.env.pnpm_run_skip_deps_check, "true")'])
})

test('multi-project workspace', async () => {
  const manifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
    },
    foo: {
      name: 'foo',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
    },
    bar: {
      name: 'bar',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
    },
  }

  const projects = preparePackages([
    {
      location: '.',
      package: manifests.root,
    },
    manifests.foo,
    manifests.bar,
  ])

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  const EXEC = ['exec', 'node', '--print', '"hello from exec: " + process.cwd()'] as const

  // attempting to execute a command in root without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_NO_CACHE')
  }
  // attempting to execute a command in a workspace package without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, ...EXEC], {
      cwd: projects.foo.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_NO_CACHE')
  }
  // attempting to execute a command recursively without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--recursive', ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_NO_CACHE')
  }
  // attempting to execute a command with filter without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--filter=foo', ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_NO_CACHE')
  }

  await execPnpm([...CONFIG, 'install'])

  // pnpm install should create a packages list cache
  {
    const workspaceState = loadWorkspaceState(process.cwd())
    expect(workspaceState).toStrictEqual({
      catalogs: {},
      lastValidatedTimestamp: expect.any(Number),
      projectRootDirs: [
        path.resolve('.'),
        path.resolve('foo'),
        path.resolve('bar'),
      ].sort(),
    })
  }

  // should be able to execute a command in root after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain(`hello from exec: ${process.cwd()}`)
  }
  // should be able to execute a command in a workspace package after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], {
      cwd: projects.foo.dir(),
      expectSuccess: true,
    })
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('foo')}`)
  }
  // should be able to execute a command recursively after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, '--recursive', ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('foo')}`)
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('bar')}`)
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('.')}`)
  }
  // should be able to execute a command with filter after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, '--filter=foo', ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('foo')}`)
  }

  projects.foo.writePackageJson(manifests.foo)

  // if the mtime of one manifest file changes but its content doesn't, pnpm run should update the packages list then run the script normally
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain(`hello from exec: ${process.cwd()}`)
  }
  // should skip check after pnpm has updated the packages list
  {
    const { stdout } = execPnpmSync([...CONFIG, '--reporter=ndjson', ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain(`hello from exec: ${process.cwd()}`)
  }

  projects.foo.writePackageJson({
    ...manifests.foo,
    dependencies: {
      ...manifests.foo.dependencies,
      '@pnpm.e2e/foo': '=100.1.0',
    },
  } as ProjectManifest)

  // attempting to execute a command in root without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
  }
  // attempting to execute a command in any workspace package without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, ...EXEC], {
      cwd: projects.foo.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
  }
  {
    const { status, stdout } = execPnpmSync([...CONFIG, ...EXEC], {
      cwd: projects.bar.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
  }
  // attempting to execute a command recursively without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--recursive', ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
  }
  // attempting to execute a command with filter without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--filter=foo', ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
  }

  await execPnpm([...CONFIG, 'install'])

  // should be able to execute a command in root after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain(`hello from exec: ${process.cwd()}`)
  }
  // should be able to execute a command in any workspace package after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], {
      cwd: projects.foo.dir(),
      expectSuccess: true,
    })
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('foo')}`)
  }
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], {
      cwd: projects.bar.dir(),
      expectSuccess: true,
    })
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('bar')}`)
  }
  // should be able to execute a command recursively after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, '--recursive', ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('foo')}`)
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('bar')}`)
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('.')}`)
  }
  // should be able to execute a command with filter after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, '--filter=foo', ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain(`hello from exec: ${path.resolve('foo')}`)
  }

  manifests.baz = {
    name: 'bar',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': '=100.0.0',
    },
  }
  fs.mkdirSync(path.resolve('baz'), { recursive: true })
  fs.writeFileSync(path.resolve('baz/package.json'), JSON.stringify(manifests.baz, undefined, 2) + '\n')

  // attempting to execute a command without updating projects list should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, ...EXEC])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_WORKSPACE_STRUCTURE_CHANGED')
  }

  await execPnpm([...CONFIG, 'install'])

  // pnpm install should update the packages list cache
  {
    const workspaceState = loadWorkspaceState(process.cwd())
    expect(workspaceState).toStrictEqual({
      catalogs: {},
      lastValidatedTimestamp: expect.any(Number),
      projectRootDirs: [
        path.resolve('.'),
        path.resolve('foo'),
        path.resolve('bar'),
        path.resolve('baz'),
      ].sort(),
    })
  }

  // should be able to execute a command after projects list have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, ...EXEC], { expectSuccess: true })
    expect(stdout.toString()).toContain(`hello from exec: ${process.cwd()}`)
  }

  // should set env.pnpm_run_skip_deps_check for all the scripts
  await execPnpm([...CONFIG, 'exec', 'node', '--eval', 'assert.strictEqual(process.env.pnpm_run_skip_deps_check, "true")'])
})
