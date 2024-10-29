import fs from 'fs'
import path from 'path'
import { prepare } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { loadPackagesList } from '@pnpm/workspace.packages-list-cache'
import { execPnpm, execPnpmSync, pnpmBinLocation } from '../utils'

const CHECK_DEPS_BEFORE_RUN_SCRIPTS = '--config.check-deps-before-run-scripts=true'

test('single dependency', async () => {
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
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from script')
  }

  project.writePackageJson(manifest)

  // should be able to execute a script after the mtime of the manifest change but the content doesn't
  {
    const { stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'], { expectSuccess: true })
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
    const { stdout } = execPnpmSync([...config, '--reporter=ndjson', 'start'], { expectSuccess: true })
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
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from script')
  }

  // should set env.pnpm_run_skip_deps_check for the script
  await execPnpm([...config, 'run', 'checkEnv'])
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
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
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
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
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

  // it shouldn't fail after the dependencies have been updated
  {
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('manifest mutated')
    expect(stdout.toString()).toContain('hello from the nested script')
  }

  // it shouldn't fail after the manifest has been rewritten with the same content
  {
    const { stdout } = execPnpmSync([...config, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('manifest mutated')
    expect(stdout.toString()).toContain('hello from the nested script')
  }
})
