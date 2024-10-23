import path from 'path'
import { prepare } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { loadPackagesList } from '@pnpm/workspace.packages-list-cache'
import { execPnpm, execPnpmSync } from '../utils'

const CHECK_DEPS_BEFORE_RUN_SCRIPTS = '--config.check-deps-before-run-scripts=true'

test('should check for outdated dependencies for single project', async () => {
  const manifest: ProjectManifest = {
    name: 'root',
    private: true,
    dependencies: {
      '@pnpm.e2e/foo': '100.0.0',
    },
    scripts: {
      start: 'echo hello from script',
      'check-env:linux': 'echo pnpm_run_skip_deps_check is $pnpm_run_skip_deps_check',
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

  project.writePackageJson({
    ...manifest,
    dependencies: {
      ...manifest.dependencies,
      '@pnpm.e2e/foo': '100.1.0',
    },
  })

  // attempting to execute a script with outdated dependencies should fail
  {
    const { status, stdout } = execPnpmSync([...config, 'start'])
    expect(status).not.toBe(0)
    expect(stdout.toString()).toContain('ERR_PNPM_RUN_CHECK_DEPS_UNSATISFIED_PKG_MANIFEST')
  }

  await execPnpm([...config, 'install'])

  // should be able to execute a script after dependencies have been updated
  {
    const { status, stdout } = execPnpmSync([...config, 'start'])
    expect(status).toBe(0)
    expect(stdout.toString()).toContain('hello from script')
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
  if (process.platform === 'linux') {
    const { status, stdout } = execPnpmSync([...config, 'run', 'check-env:linux'])
    expect(status).toBe(0)
    expect(stdout.toString()).toContain('pnpm_run_skip_deps_check is true')
  }
})

test.todo('single project with no dependencies')

// test.todo('should not check if env.pnpm_run_skip_deps_check is defined')

test.todo('should not prevent nested `pnpm run` after having mutated the manifests')

test.todo('should check for outdated dependencies for multi-project workspace before `pnpm run` on the root package')

test.todo('should check for outdated dependencies for multi-project workspace before `pnpm run` on one of the package in the workspace')

test.todo('should check for outdated dependencies for multi-project workspace before recursive run')
