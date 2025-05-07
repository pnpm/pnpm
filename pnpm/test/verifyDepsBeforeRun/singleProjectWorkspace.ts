import fs from 'fs'
import path from 'path'
import { prepare } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { loadWorkspaceState } from '@pnpm/workspace.state'
import { execPnpm, execPnpmSync, pnpmBinLocation } from '../utils'

const CONFIG = ['--config.verify-deps-before-run=error'] as const

test('single dependency', async () => {
  const manifest: ProjectManifest = {
    name: 'root',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
    },
    scripts: {
      start: 'echo hello from script',
      checkEnv: 'node --eval "assert.strictEqual(process.env.npm_config_verify_deps_before_run, \'false\')"',
    },
  }

  const project = prepare(manifest)

  // attempting to execute a script without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...CONFIG, 'install'])

  // installing dependencies on a single package workspace should create a packages list cache
  const workspaceState = loadWorkspaceState(process.cwd())
  expect(workspaceState).toBeDefined()

  // should be able to execute a script after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from script')
  }

  project.writePackageJson(manifest)

  // should be able to execute a script after the mtime of the manifest change but the content doesn't
  {
    const { stdout } = execPnpmSync([...CONFIG, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from script')
    expect(stdout.toString()).not.toContain('The manifest file is not newer than the lockfile. Exiting check.')
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
    const { status, stdout } = execPnpmSync([...CONFIG, '--reporter=ndjson', 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
    expect(stdout.toString()).not.toContain('The manifest file is not newer than the lockfile. Exiting check.')
    expect(stdout.toString()).toContain('The manifest is newer than the lockfile. Continuing check.')
  }

  await execPnpm([...CONFIG, 'install'])

  // should be able to execute a script after dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, '--reporter=ndjson', 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from script')
    expect(stdout.toString()).toContain('The manifest file is not newer than the lockfile. Exiting check.')
    expect(stdout.toString()).not.toContain('The manifest is newer than the lockfile. Continuing check.')
  }

  project.writePackageJson({
    ...manifest,
    dependencies: {}, // delete all dependencies
  })

  // attempting to execute a script with redundant dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
  }

  await execPnpm([...CONFIG, 'install'])

  // should be able to execute a script without dependencies
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from script')
  }

  // should set env.npm_config_verify_deps_before_run to false for the script (to skip check in nested script)
  await execPnpm([...CONFIG, 'run', 'checkEnv'])
})

test('deleting node_modules after install', async () => {
  const manifest: ProjectManifest = {
    name: 'root',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
    },
    scripts: {
      start: 'echo hello from script',
    },
  }

  prepare(manifest)

  // attempting to execute a script without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
  }

  await execPnpm([...CONFIG, 'install'])

  // installing dependencies on a single package workspace should create a packages list cache
  const workspaceState = loadWorkspaceState(process.cwd())
  expect(workspaceState).toBeDefined()

  // should be able to execute a script after dependencies have been installed
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('hello from script')
  }

  fs.rmSync('node_modules', { recursive: true })

  // attempting to execute a script after node_modules has been deleted should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
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

  // attempting to execute a script without the lockfile should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...CONFIG, 'install'])

  // installing dependencies on a single package workspace should create a packages list cache
  const workspaceState = loadWorkspaceState(process.cwd())
  expect(workspaceState).toBeDefined()

  // should be able to execute a script after the lockfile has been created
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
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
  fs.writeFileSync('.npmrc', 'verify-deps-before-run=error', 'utf8')

  const cacheDir = path.resolve('cache')

  // add a script named `start` which would inherit `config` and invoke `nestedScript`
  manifest.scripts!.start =
    `node mutate-manifest.js && node ${pnpmBinLocation} --config.cache-dir=${cacheDir} run nestedScript`
  project.writePackageJson(manifest)

  // attempting to execute a script without installing dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('Cannot check whether dependencies are outdated')
  }

  await execPnpm([...CONFIG, 'install'])

  // mutating the manifest should not cause nested `pnpm run nestedScript` to fail
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('manifest mutated')
    expect(stdout.toString()).toContain('hello from the nested script')
  }

  // non nested script (`start`) should still fail (after `nestedScript` modified the manifest)
  {
    const { status, stdout } = execPnpmSync([...CONFIG, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_VERIFY_DEPS_BEFORE_RUN')
  }

  await execPnpm([...CONFIG, 'install'])

  // it shouldn't fail after the dependencies have been updated
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('manifest mutated')
    expect(stdout.toString()).toContain('hello from the nested script')
  }

  // it shouldn't fail after the manifest has been rewritten with the same content (by `nestedScript`)
  {
    const { stdout } = execPnpmSync([...CONFIG, 'start'], { expectSuccess: true })
    expect(stdout.toString()).toContain('manifest mutated')
    expect(stdout.toString()).toContain('hello from the nested script')
  }
})
