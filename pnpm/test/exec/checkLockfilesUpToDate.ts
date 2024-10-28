import fs from 'fs'
import path from 'path'
import { prepare, preparePackages } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { loadPackagesList } from '@pnpm/workspace.packages-list-cache'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync, pnpmBinLocation } from '../utils'

const CHECK_DEPS_BEFORE_RUN_SCRIPTS = '--config.check-deps-before-run-scripts=true'

describe('single project workspace', () => {
  test.only('single dependency', async () => {
    const manifest: ProjectManifest = {
      name: 'root',
      private: true,
      dependencies: {
        '@pnpm.e2e/foo': '100.0.0',
      },
      scripts: {
        start: 'echo hello from script',
        checkEnv: 'node --eval "assert.strictEqual(process.env.pnpm_run_skip_deps_check, \'true\')"',
      },
    }

    const project = prepare(manifest)

    const cacheDir = path.resolve('cache')
    const config = [
      CHECK_DEPS_BEFORE_RUN_SCRIPTS,
      `--config.cache-dir=${cacheDir}`,
    ]

    // attempting to execute a script without installing dependencies should fail
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).not.toBe(0)
      expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND')
    }

    await execPnpm([...config, 'install'])

    // installing dependencies on a single package workspace should not create a packages list cache
    {
      const packagesList = await loadPackagesList({ cacheDir, workspaceDir: process.cwd() })
      expect(packagesList).toBeUndefined()
    }

    // should be able to execute a script after dependencies have been installed
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from script')
    }

    project.writePackageJson(manifest)

    // should be able to execute a script after the mtime of the manifest change but the content doesn't
    {
      const { status, stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from script')
      expect(stdout.toString()).not.toContain('The manifest file not newer than the lockfile. Exiting check.')
      expect(stdout.toString()).toContain('The manifest is newer than the lockfile. Continuing check.')
    }

    project.writePackageJson({
      ...manifest,
      dependencies: {
        ...manifest.dependencies,
        '@pnpm.e2e/foo': '100.1.0',
      },
    })

    // attempting to execute a script with outdated dependencies should fail
    {
      const { status, stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'])
      expect(status).not.toBe(0)
      expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
      expect(stdout.toString()).not.toContain('The manifest file not newer than the lockfile. Exiting check.')
      expect(stdout.toString()).toContain('The manifest is newer than the lockfile. Continuing check.')
    }

    await execPnpm([...config, 'install'])

    // should be able to execute a script after dependencies have been updated
    {
      const { status, stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from script')
      expect(stdout.toString()).toContain('The manifest file not newer than the lockfile. Exiting check.')
      expect(stdout.toString()).not.toContain('The manifest is newer than the lockfile. Continuing check.')
    }

    project.writePackageJson({
      ...manifest,
      dependencies: {}, // delete all dependencies
    })

    // attempting to execute a script with redundant dependencies should fail
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).not.toBe(0)
      expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    }

    await execPnpm([...config, 'install'])

    // should be able to execute a script without dependencies
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from script')
    }

    // should set env.pnpm_run_skip_deps_check for the script
    {
      const { status } = execPnpmSync([...config, 'run', 'checkEnv'])
      expect(status).toBe(0)
    }
  })

  test('no dependencies', async () => {
    const manifest: ProjectManifest = {
      name: 'root',
      private: true,
      scripts: {
        start: 'echo hello from script',
      },
    }

    prepare(manifest)

    const cacheDir = path.resolve('cache')
    const config = [
      CHECK_DEPS_BEFORE_RUN_SCRIPTS,
      `--config.cache-dir=${cacheDir}`,
    ]

    // attempting to execute a script without the lockfile should fail
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).not.toBe(0)
      expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND')
    }

    await execPnpm([...config, 'install'])

    // installing dependencies on a single package workspace should not create a packages list cache
    {
      const packagesList = await loadPackagesList({ cacheDir, workspaceDir: process.cwd() })
      expect(packagesList).toBeUndefined()
    }

    // should be able to execute a script after the lockfile has been created
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from script')
    }
  })

  test('nested `pnpm run` should not check for mutated manifest', async () => {
    const manifest: ProjectManifest = {
      name: 'root',
      private: true,
      scripts: {
        nestedScript: 'echo hello from the nested script',
      },
      dependencies: {
        '@pnpm.e2e/foo': '100.0.0',
      },
    }

    const project = prepare(manifest)

    fs.writeFileSync('mutate-manifest.js', `
      const fs = require('fs')
      const manifest = require('./package.json')
      manifest.dependencies['@pnpm.e2e/foo'] = '100.1.0'
      const jsonText = JSON.stringify(manifest, undefined, 2)
      fs.writeFileSync(require.resolve('./package.json'), jsonText)
      console.log('manifest mutated')
    `)

    const cacheDir = path.resolve('cache')
    const config = [
      CHECK_DEPS_BEFORE_RUN_SCRIPTS,
      `--config.cache-dir=${cacheDir}`,
    ]

    // add a script named `start` which would inherit `config` and invoke `nestedScript`
    manifest.scripts!.start =
      `${process.execPath} mutate-manifest.js && ${process.execPath} ${pnpmBinLocation} ${config.join(' ')} run nestedScript`
    project.writePackageJson(manifest)

    // attempting to execute a script without installing dependencies should fail
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).not.toBe(0)
      expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_LOCKFILE_NOT_FOUND')
    }

    await execPnpm([...config, 'install'])

    // mutating the manifest should not cause nested `pnpm run nestedScript` to fail
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('manifest mutated')
      expect(stdout.toString()).toContain('hello from the nested script')
    }

    // non nested script should still fail
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).not.toBe(0)
      expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
    }

    await execPnpm([...config, 'install'])

    // it shouldn't fail after having update the dependencies
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('manifest mutated')
      expect(stdout.toString()).toContain('hello from the nested script')
    }

    // it shouldn't fail after manifest having been rewritten with the same content
    {
      const { status, stdout } = execPnpmSync([...config, 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('manifest mutated')
      expect(stdout.toString()).toContain('hello from the nested script')
    }
  })
})

describe('multi-project workspace', () => {
  test('single dependency', async () => {
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

    // should be able to execute a script in root after dependencies having been installed
    {
      const { status, stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from root')
      expect(stdout.toString()).toContain('No manifest files are modified after the last validation. Exiting check.')
      expect(stdout.toString()).not.toContain('Some manifest files are modified after the last validation. Continuing check.')
    }
    // should be able to execute a script in a workspace package after dependencies having been installed
    {
      const { status, stdout } = execPnpmSync([...config, 'start'], {
        cwd: projects.foo.dir(),
      })
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from foo')
    }
    // should be able to execute a script recursively after dependencies having been installed
    {
      const { status, stdout } = execPnpmSync([...config, '--recursive', 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from foo')
      expect(stdout.toString()).toContain('hello from bar')
    }
    // should be able to execute a script with filter after dependencies having been installed
    {
      const { status, stdout } = execPnpmSync([...config, '--filter=foo', 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from foo')
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

    // should be able to execute a script in root after updating dependencies
    {
      const { status, stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from root')
      expect(stdout.toString()).toContain('No manifest files are modified after the last validation. Exiting check.')
      expect(stdout.toString()).not.toContain('Some manifest files are modified after the last validation. Continuing check.')
    }
    // should be able to a script in any workspace package after updating dependencies
    {
      const { status, stdout } = execPnpmSync([...config, 'start'], {
        cwd: projects.foo.dir(),
      })
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from foo')
    }
    {
      const { status, stdout } = execPnpmSync([...config, 'start'], {
        cwd: projects.bar.dir(),
      })
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from bar')
    }
    // should be able to a script recursively after updating dependencies
    {
      const { status, stdout } = execPnpmSync([...config, '--recursive', 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from foo')
      expect(stdout.toString()).toContain('hello from bar')
    }
    // should be able to a script with filter after updating dependencies
    {
      const { status, stdout } = execPnpmSync([...config, '--filter=foo', 'start'])
      expect(status).toBe(0)
      expect(stdout.toString()).toContain('hello from foo')
    }
  })

  test.todo('should not prevent nested `pnpm run` after having mutated the manifests')

  test.todo('should check for outdated dependencies before `pnpm run` on the root package')

  test.todo('should check for outdated dependencies before `pnpm run` on one of the package in the workspace')

  test.todo('should check for outdated dependencies before recursive run')

  test.todo('should check for outdated catalogs')
})
