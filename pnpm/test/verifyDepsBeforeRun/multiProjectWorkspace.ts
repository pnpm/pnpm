import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { loadWorkspaceState } from '@pnpm/workspace.state'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync, pnpmBinLocation } from '../utils'

const CONFIG = ['--config.verify-deps-before-run=error'] as const

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

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  // attempting to execute a script in root without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }
  // attempting to execute a script in a workspace package without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'], {
      cwd: projects.foo.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }
  // attempting to execute a script recursively without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--recursive', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }
  // attempting to execute a script with filter without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--filter=foo', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...CONFIG, 'install'])

  // pnpm install should create a packages list cache
  {
    const workspaceState = loadWorkspaceState(process.cwd())
    expect(workspaceState).toStrictEqual(expect.objectContaining({
      lastValidatedTimestamp: expect.any(Number),
      pnpmfileExists: false,
      filteredInstall: false,
      projects: {
        [path.resolve('.')]: { name: 'root', version: '0.0.0' },
        [path.resolve('foo')]: { name: 'foo', version: '0.0.0' },
        [path.resolve('bar')]: { name: 'bar', version: '0.0.0' },
      },
    }))
  }

  // should be able to execute a script in root after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).toContain('No manifest files were modified since the last validation. Exiting check.')
    expect(stdout.toString()).not.toContain('Some manifest files were modified since the last validation. Continuing check.')
  }
  // should be able to execute a script in a workspace package after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], {
      cwd: projects.foo.dir(),
      expectSuccess: true,
    })
    expect(stdout.toString()).toContain('hello from foo')
  }
  // should be able to execute a script recursively after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, '--recursive', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
    expect(stdout.toString()).toContain('hello from bar')
  }
  // should be able to execute a script with filter after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, '--filter=foo', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
  }

  projects.foo.writePackageJson(manifests.foo)

  // if the mtime of one manifest file changes but its content doesn't, pnpm run should update the packages list then run the script normally
  {
    const { stdout } = execPnpmSync([...CONFIG, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).not.toContain('No manifest files were modified since the last validation. Exiting check.')
    expect(stdout.toString()).toContain('Some manifest files were modified since the last validation. Continuing check.')
    expect(stdout.toString()).toContain('updating workspace state')
  }
  // should skip check after pnpm has updated the packages list
  {
    const { stdout } = execPnpmSync([...CONFIG, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).toContain('No manifest files were modified since the last validation. Exiting check.')
    expect(stdout.toString()).not.toContain('Some manifest files were modified since the last validation. Continuing check.')
    expect(stdout.toString()).not.toContain('updating workspace state')
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
    const { status, stdout } = execPnpmSync([...CONFIG, '--reporter=ndjson', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain('project of id foo')
    expect(stdout.toString()).not.toContain('No manifest files were modified since the last validation. Exiting check.')
    expect(stdout.toString()).toContain('Some manifest files were modified since the last validation. Continuing check.')
  }
  // attempting to execute a script in any workspace package without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'], {
      cwd: projects.foo.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain('project of id foo')
  }
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'], {
      cwd: projects.bar.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain('project of id foo')
  }
  // attempting to execute a script recursively without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--recursive', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain('project of id foo')
  }
  // attempting to execute a script with filter without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--filter=foo', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain('project of id foo')
  }

  await execPnpm([...CONFIG, 'install'])

  // should be able to execute a script in root after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).toContain('No manifest files were modified since the last validation. Exiting check.')
    expect(stdout.toString()).not.toContain('Some manifest files were modified since the last validation. Continuing check.')
  }
  // should be able to execute a script in any workspace package after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], {
      cwd: projects.foo.dir(),
      expectSuccess: true,
    })
    expect(stdout.toString()).toContain('hello from foo')
  }
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], {
      cwd: projects.bar.dir(),
      expectSuccess: true,
    })
    expect(stdout.toString()).toContain('hello from bar')
  }
  // should be able to execute a script recursively after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, '--recursive', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
    expect(stdout.toString()).toContain('hello from bar')
  }
  // should be able to execute a script with filter after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, '--filter=foo', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
  }

  manifests.baz = {
    name: 'baz',
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
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('The workspace structure has changed since last install')
  }

  await execPnpm([...CONFIG, 'install'])

  // pnpm install should update the packages list cache
  {
    const workspaceState = loadWorkspaceState(process.cwd())
    expect(workspaceState).toStrictEqual(expect.objectContaining({
      lastValidatedTimestamp: expect.any(Number),
      pnpmfileExists: false,
      filteredInstall: false,
      projects: {
        [path.resolve('.')]: { name: 'root', version: '0.0.0' },
        [path.resolve('foo')]: { name: 'foo' },
        [path.resolve('bar')]: { name: 'bar', version: '0.0.0' },
        [path.resolve('baz')]: { name: 'baz' },
      },
    }))
  }

  // should be able to execute a script after projects list have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
  }

  // should set env.pnpm_run_skip_deps_check for all the scripts
  await execPnpm([...CONFIG, '--recursive', 'run', 'checkEnv'])
})

test('multiple lockfiles', async () => {
  const manifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
      scripts: {
        start: 'echo hello from root',
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

  const config = [
    ...CONFIG,
    '--config.shared-workspace-lockfile=false',
  ]

  // attempting to execute a script in root without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }
  // attempting to execute a script in a workspace package without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'], {
      cwd: projects.foo.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }
  // attempting to execute a script recursively without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--recursive', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }
  // attempting to execute a script with filter without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--filter=foo', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...config, 'install'])

  // pnpm install should create a packages list cache
  {
    const workspaceState = loadWorkspaceState(process.cwd())
    expect(workspaceState).toStrictEqual(expect.objectContaining({
      lastValidatedTimestamp: expect.any(Number),
      pnpmfileExists: false,
      filteredInstall: false,
      projects: {
        [path.resolve('.')]: { name: 'root', version: '0.0.0' },
        [path.resolve('foo')]: { name: 'foo', version: '0.0.0' },
        [path.resolve('bar')]: { name: 'bar', version: '0.0.0' },
      },
    }))
  }

  // should be able to execute a script in root after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).toContain('No manifest files were modified since the last validation. Exiting check.')
    expect(stdout.toString()).not.toContain('Some manifest files were modified since the last validation. Continuing check.')
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
    expect(stdout.toString()).not.toContain('No manifest files were modified since the last validation. Exiting check.')
    expect(stdout.toString()).toContain('Some manifest files were modified since the last validation. Continuing check.')
    expect(stdout.toString()).toContain('updating workspace state')
  }
  // should skip check after pnpm has updated the packages list
  {
    const { stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
    expect(stdout.toString()).toContain('No manifest files were modified since the last validation. Exiting check.')
    expect(stdout.toString()).not.toContain('Some manifest files were modified since the last validation. Continuing check.')
    expect(stdout.toString()).not.toContain('updating workspace state')
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
    const { status, stdout } = execPnpmSync([...config, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain(`The lockfile in ${path.resolve('foo')} does not satisfy project of id .`)
  }
  // attempting to execute a script in any workspace package without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'], {
      cwd: projects.foo.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain(`The lockfile in ${path.resolve('foo')} does not satisfy project of id .`)
  }
  {
    const { status, stdout } = execPnpmSync([...config, 'start'], {
      cwd: projects.bar.dir(),
    })
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain(`The lockfile in ${path.resolve('foo')} does not satisfy project of id .`)
  }
  // attempting to execute a script recursively without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--recursive', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain(`The lockfile in ${path.resolve('foo')} does not satisfy project of id .`)
  }
  // attempting to execute a script with filter without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, '--filter=foo', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).toContain(`The lockfile in ${path.resolve('foo')} does not satisfy project of id .`)
  }

  await execPnpm([...config, 'install'])

  // should be able to execute a script in root after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
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
    name: 'baz',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': '=100.0.0',
    },
    scripts: {
      start: 'echo hello from baz',
    },
  }
  fs.mkdirSync(path.resolve('baz'), { recursive: true })
  fs.writeFileSync(path.resolve('baz/package.json'), JSON.stringify(manifests.baz, undefined, 2) + '\n')

  // attempting to execute a script without updating projects list should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('The workspace structure has changed since last install')
  }

  await execPnpm([...config, 'install'])

  // pnpm install should update the packages list cache
  {
    const workspaceState = loadWorkspaceState(process.cwd())
    expect(workspaceState).toStrictEqual(expect.objectContaining({
      lastValidatedTimestamp: expect.any(Number),
      pnpmfileExists: false,
      filteredInstall: false,
      projects: {
        [path.resolve('.')]: { name: 'root', version: '0.0.0' },
        [path.resolve('foo')]: { name: 'foo' },
        [path.resolve('bar')]: { name: 'bar', version: '0.0.0' },
        [path.resolve('baz')]: { name: 'baz' },
      },
    }))
  }

  // should be able to execute a script after projects list have been updated
  {
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
  }
})

test('filtered install', async () => {
  const manifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
      scripts: {
        start: 'echo hello from root',
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

  // attempting to execute a script without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--filter=foo', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...CONFIG, '--filter=foo', 'install'])

  // should be able to execute a script after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, '--filter=foo', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
  }

  manifests.foo.dependencies!['@pnpm.e2e/foo'] = '=100.1.0'
  projects.foo.writePackageJson(manifests.foo)

  // attempt to execute a script without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--filter=foo', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
  }

  await execPnpm([...CONFIG, '--filter=foo', 'install'])

  // should be able to execute a script after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, '--filter=foo', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from foo')
  }
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

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  // attempting to execute a script without `pnpm install` should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...CONFIG, 'install'])

  // pnpm install should create a packages list cache
  {
    const workspaceState = loadWorkspaceState(process.cwd())
    expect(workspaceState).toStrictEqual(expect.objectContaining({
      lastValidatedTimestamp: expect.any(Number),
      pnpmfileExists: false,
      filteredInstall: false,
      projects: {
        [path.resolve('.')]: { name: 'root', version: '0.0.0' },
        [path.resolve('foo')]: { name: 'foo', version: '0.0.0' },
        [path.resolve('bar')]: { name: 'bar', version: '0.0.0' },
      },
    }))
  }

  // should be able to execute a script after `pnpm install`
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
  }
})

test('nested `pnpm run` should not check for mutated manifest', async () => {
  const manifests: Record<string, ProjectManifest> = {
    foo: {
      name: 'foo',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
      scripts: {
        nestedScript: 'echo hello from nested script of foo',
      },
    },
    bar: {
      name: 'bar',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
      scripts: {
        nestedScript: 'echo hello from nested script of bar',
      },
    },
  }

  const projects = preparePackages([
    manifests.foo,
    manifests.bar,
  ])

  for (const name in projects) {
    const scriptPath = path.join(projects[name].dir(), 'mutate-manifest.js')
    fs.writeFileSync(scriptPath, `
      const fs = require('fs')
      const manifest = require('./package.json')
      manifest.dependencies['@pnpm.e2e/foo'] = '100.1.0'
      const jsonText = JSON.stringify(manifest, undefined, 2)
      fs.writeFileSync(require.resolve('./package.json'), jsonText)
      console.log('manifest mutated: ${name}')
    `)
  }

  // add to every manifest file a script named `start` which would inherit `config` and invoke `nestedScript`
  for (const name in projects) {
    manifests[name].scripts!.start =
      `node mutate-manifest.js && node ${pnpmBinLocation} ${CONFIG.join(' ')} run nestedScript`
    projects[name].writePackageJson(manifests[name])
  }

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  // attempting to execute a script without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--recursive', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...CONFIG, 'install'])

  // mutating the manifest should not cause nested `pnpm run nestedScript` to fail
  {
    const { stdout } = execPnpmSync([...CONFIG, '--recursive', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('manifest mutated: foo')
    expect(stdout.toString()).toContain('hello from nested script of foo')
    expect(stdout.toString()).toContain('manifest mutated: bar')
    expect(stdout.toString()).toContain('hello from nested script of bar')
  }

  // non nested script (`start`) should still fail (after `nestedScript` modified the manifests)
  {
    const { status, stdout } = execPnpmSync([...CONFIG, '--recursive', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
  }

  await execPnpm([...CONFIG, 'install'])

  // it shouldn't fail after the dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, '--recursive', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('manifest mutated: foo')
    expect(stdout.toString()).toContain('hello from nested script of foo')
    expect(stdout.toString()).toContain('manifest mutated: bar')
    expect(stdout.toString()).toContain('hello from nested script of bar')
  }

  // it shouldn't fail after the manifests have been rewritten with the same content (by `nestedScript`)
  {
    const { stdout } = execPnpmSync([...CONFIG, '--recursive', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('manifest mutated: foo')
    expect(stdout.toString()).toContain('hello from nested script of foo')
    expect(stdout.toString()).toContain('manifest mutated: bar')
    expect(stdout.toString()).toContain('hello from nested script of bar')
  }
})

test('should check for outdated catalogs', async () => {
  const manifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
      scripts: {
        start: 'echo hello from root',
      },
    },
    foo: {
      name: 'foo',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
      scripts: {
        start: 'echo hello from foo',
      },
    },
    bar: {
      name: 'bar',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': 'catalog:',
      },
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

  const workspaceManifest = {
    catalog: {
      '@pnpm.e2e/foo': '=100.0.0',
    } as Record<string, string>,
    packages: ['**', '!store/**'],
  }
  writeYamlFile('pnpm-workspace.yaml', workspaceManifest)

  // attempting to execute a script without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...CONFIG, 'install'])

  // pnpm install should create a packages list cache
  {
    const workspaceState = loadWorkspaceState(process.cwd())
    expect(workspaceState).toStrictEqual({
      settings: expect.objectContaining({
        catalogs: {
          default: workspaceManifest.catalog,
        },
      }),
      pnpmfileExists: false,
      filteredInstall: false,
      lastValidatedTimestamp: expect.any(Number),
      projects: {
        [path.resolve('.')]: { name: 'root', version: '0.0.0' },
        [path.resolve('foo')]: { name: 'foo', version: '0.0.0' },
        [path.resolve('bar')]: { name: 'bar', version: '0.0.0' },
      },
    })
  }

  // should be able to execute a script after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
  }

  workspaceManifest.catalog.foo = '=100.1.0'
  writeYamlFile('pnpm-workspace.yaml', workspaceManifest)

  // attempting to execute a script without updating dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Catalogs cache outdated')
  }

  await execPnpm([...CONFIG, 'install'])

  // should be able to execute a script after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
  }
})

test('failed to install dependencies', async () => {
  const manifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '=100.0.0',
      },
      scripts: {
        start: 'echo hello from root',
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

  await execPnpm([...CONFIG, 'install'])

  // should be able to execute a script after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from root')
  }

  // modify a manifest file to require an impossible version
  manifests.foo.dependencies!['@pnpm.e2e/foo'] = '=9999.9999.9999' // this version does not exist
  projects.foo.writePackageJson(manifests.foo)

  // should fail to install dependencies
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'install'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_NO_MATCHING_VERSION')
  }

  // attempting to execute a script without successfully updating the dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
  }
})
