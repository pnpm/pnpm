import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { loadPackagesList } from '@pnpm/workspace.packages-list-cache'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync } from '../utils'

const CHECK_DEPS_BEFORE_RUN_SCRIPTS = '--config.check-deps-before-run-scripts=true'

test('single dependency', async () => {
  const checkEnv = 'node --eval "assert.strictEqual(process.env.pnpm_run_skip_deps_check, \'true\')"'

  const manifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
      scripts: {
        start: 'echo hello from root',
        checkEnv,
      },
    },
    foo: {
      name: 'foo',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
      scripts: {
        start: 'echo hello from foo',
        checkEnv,
      },
    },
    bar: {
      name: 'bar',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
      scripts: {
        start: 'echo hello from bar',
        checkEnv,
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

  const cacheDir = path.resolve('cache')
  const config = [
    CHECK_DEPS_BEFORE_RUN_SCRIPTS,
    `--config.cache-dir=${cacheDir}`,
  ]

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  // attempting to execute a script in root without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_NO_CACHE')
  }
  // attempting to execute a script in a workspace package without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'], {
      cwd: projects.foo.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_NO_CACHE')
  }
  // attempting to execute a script recursively without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--recursive', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_NO_CACHE')
  }
  // attempting to execute a script with filter without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--filter=foo', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_NO_CACHE')
  }

  await execPnpm([...config, 'install'])

  // pnpm install should create a packages list cache
  {
    const packagesList = await loadPackagesList({ cacheDir, workspaceDir: process.cwd() })
    expect(packagesList).toStrictEqual({
      lastValidatedTimestamp: expect.any(Number),
      projectRootDirs: [
        path.resolve('.'),
        path.resolve('foo'),
        path.resolve('bar'),
      ].sort(),
      workspaceDir: process.cwd(),
    })
  }

  // should be able to execute a script in root after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).toContain('No manifest files are modified after the last validation. Exiting check.')
    expect(stdout.toString()).not.toContain('Some manifest files are modified after the last validation. Continuing check.')
  }
  // should be able to execute a script in a workspace package after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...config, 'start'], {
      cwd: projects.foo.dir(),
      expectSuccess: true,
    })
    expect(stdout.toString()).toContain('hello from foo')
  }
  // should be able to execute a script recursively after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...config, '--recursive', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
    expect(stdout.toString()).toContain('hello from bar')
  }
  // should be able to execute a script with filter after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...config, '--filter=foo', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
  }

  projects.foo.writePackageJson(manifests.foo)

  // if the mtime of one manifest file changes but its content doesn't, pnpm run should update the packages list then run the script normally
  {
    const { stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).not.toContain('No manifest files are modified after the last validation. Exiting check.')
    expect(stdout.toString()).toContain('Some manifest files are modified after the last validation. Continuing check.')
    expect(stdout.toString()).toContain('updating packages list')
  }
  // should skip check after pnpm has updated the packages list
  {
    const { stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).toContain('No manifest files are modified after the last validation. Exiting check.')
    expect(stdout.toString()).not.toContain('Some manifest files are modified after the last validation. Continuing check.')
    expect(stdout.toString()).not.toContain('updating packages list')
  }

  projects.foo.writePackageJson({
    ...manifests.foo,
    dependencies: {
      ...manifests.foo.dependencies,
      '@pnpm.e2e/foo': '=100.1.0',
    },
  } as ProjectManifest)

  // attempting to execute a script in root without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
    expect(stdout.toString()).not.toContain('No manifest files are modified after the last validation. Exiting check.')
    expect(stdout.toString()).toContain('Some manifest files are modified after the last validation. Continuing check.')
  }
  // attempting to execute a script in any workspace package without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'], {
      cwd: projects.foo.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
  }
  {
    const { status, stdout } = execPnpmSync([...config, 'start'], {
      cwd: projects.bar.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
  }
  // attempting to execute a script recursively without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--recursive', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
  }
  // attempting to execute a script with filter without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--filter=foo', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    expect(stdout.toString()).toContain('project of id foo')
  }

  await execPnpm([...config, 'install'])

  // should be able to execute a script in root after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).toContain('No manifest files are modified after the last validation. Exiting check.')
    expect(stdout.toString()).not.toContain('Some manifest files are modified after the last validation. Continuing check.')
  }
  // should be able to execute a script in any workspace package after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...config, 'start'], {
      cwd: projects.foo.dir(),
      expectSuccess: true,
    })
    expect(stdout.toString()).toContain('hello from foo')
  }
  {
    const { stdout } = execPnpmSync([...config, 'start'], {
      cwd: projects.bar.dir(),
      expectSuccess: true,
    })
    expect(stdout.toString()).toContain('hello from bar')
  }
  // should be able to execute a script recursively after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...config, '--recursive', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
    expect(stdout.toString()).toContain('hello from bar')
  }
  // should be able to execute a script with filter after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...config, '--filter=foo', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
  }

  manifests.baz = {
    name: 'bar',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': '=100.0.0',
    },
    scripts: {
      start: 'echo hello from baz',
      checkEnv,
    },
  }
  fs.mkdirSync(path.resolve('baz'), { recursive: true })
  fs.writeFileSync(path.resolve('baz/package.json'), JSON.stringify(manifests.baz, undefined, 2) + '\n')

  // attempting to execute a script without updating projects list should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_WORKSPACE_STRUCTURE_CHANGED')
  }

  await execPnpm([...config, 'install'])

  // pnpm install should update the packages list cache
  {
    const packagesList = await loadPackagesList({ cacheDir, workspaceDir: process.cwd() })
    expect(packagesList).toStrictEqual({
      lastValidatedTimestamp: expect.any(Number),
      projectRootDirs: [
        path.resolve('.'),
        path.resolve('foo'),
        path.resolve('bar'),
        path.resolve('baz'),
      ].sort(),
      workspaceDir: process.cwd(),
    })
  }

  // should be able to execute a script after projects list have been updated
  {
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
  }

  // should set env.pnpm_run_skip_deps_check for all the scripts
  await execPnpm([...config, '--recursive', 'run', 'checkEnv'])
})

test('no dependencies', async () => {
  const manifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      private: true,
      scripts: {
        start: 'echo hello from root',
      },
    },
    foo: {
      name: 'foo',
      private: true,
      scripts: {
        start: 'echo hello from foo',
      },
    },
    bar: {
      name: 'bar',
      private: true,
      scripts: {
        start: 'echo hello from bar',
      },
    },
  }

  preparePackages([
    {
      location: '.',
      package: manifests.root,
    },
    manifests.foo,
    manifests.bar,
  ])

  const cacheDir = path.resolve('cache')
  const config = [
    CHECK_DEPS_BEFORE_RUN_SCRIPTS,
    `--config.cache-dir=${cacheDir}`,
  ]

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  // attempting to execute a script without `pnpm install` should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_NO_CACHE')
  }

  await execPnpm([...config, 'install'])

  // pnpm install should create a packages list cache
  {
    const packagesList = await loadPackagesList({ cacheDir, workspaceDir: process.cwd() })
    expect(packagesList).toStrictEqual({
      lastValidatedTimestamp: expect.any(Number),
      projectRootDirs: [
        path.resolve('.'),
        path.resolve('foo'),
        path.resolve('bar'),
      ].sort(),
      workspaceDir: process.cwd(),
    })
  }

  // should be able to execute a script after `pnpm install`
  {
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
  }
})

test.todo('should not prevent nested `pnpm run` after having mutated the manifests')

test.todo('should check for outdated catalogs')
