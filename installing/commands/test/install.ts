import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'
import { add, install } from '@pnpm/installing.commands'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { rimrafSync } from '@zkochan/rimraf'
import delay from 'delay'
import { loadJsonFileSync } from 'load-json-file'

import { DEFAULT_OPTS } from './utils/index.js'

const describeOnLinuxOnly = process.platform === 'linux' ? describe : describe.skip

test('install fails if no package.json is found', async () => {
  prepareEmpty()

  await expect(install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })).rejects.toThrow(/No package\.json found/)
})

test('install does not fail when a new package is added', async () => {
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  const pkg = loadJsonFileSync<{ dependencies: Record<string, string> }>(path.resolve('package.json'))

  expect(pkg?.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
})

test('install with no store integrity validation', async () => {
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  // We should have a short delay before modifying the file in the store.
  // Otherwise pnpm will not consider it to be modified.
  await delay(200)
  const readmePath = path.join(DEFAULT_OPTS.storeDir, STORE_VERSION, 'files/9a/f6af85f55c111108eddf1d7ef7ef224b812e7c7bfabae41c79cf8bc9a910352536963809463e0af2799abacb975f22418a35a1d170055ef3fdc3b2a46ef1c5')
  fs.writeFileSync(readmePath, 'modified', 'utf8')

  rimrafSync('node_modules')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    verifyStoreIntegrity: false,
  })

  expect(fs.readFileSync('node_modules/is-positive/readme.md', 'utf8')).toBe('modified')
})

// Covers https://github.com/pnpm/pnpm/issues/7362
describeOnLinuxOnly('filters optional dependencies based on pnpm.supportedArchitectures.libc', () => {
  test.each([
    ['glibc', '@pnpm.e2e+only-linux-x64-glibc@1.0.0', '@pnpm.e2e+only-linux-x64-musl@1.0.0'],
    ['musl', '@pnpm.e2e+only-linux-x64-musl@1.0.0', '@pnpm.e2e+only-linux-x64-glibc@1.0.0'],
  ])('%p → installs %p, does not install %p', async (libc, found, notFound) => {
    const rootProjectManifest = {
      dependencies: {
        '@pnpm.e2e/support-different-architectures': '1.0.0',
      },
    }

    prepare(rootProjectManifest)

    await install.handler({
      ...DEFAULT_OPTS,
      rootProjectManifest,
      dir: process.cwd(),
      supportedArchitectures: {
        os: ['linux'],
        cpu: ['x64'],
        libc: [libc],
      },
    })

    const pkgDirs = fs.readdirSync(path.resolve('node_modules', '.pnpm'))
    expect(pkgDirs).toContain('@pnpm.e2e+support-different-architectures@1.0.0')
    expect(pkgDirs).toContain(found)
    expect(pkgDirs).not.toContain(notFound)
  })
})

describeOnLinuxOnly('filters optional dependencies based on --libc', () => {
  test.each([
    ['glibc', '@pnpm.e2e+only-linux-x64-glibc@1.0.0', '@pnpm.e2e+only-linux-x64-musl@1.0.0'],
    ['musl', '@pnpm.e2e+only-linux-x64-musl@1.0.0', '@pnpm.e2e+only-linux-x64-glibc@1.0.0'],
  ])('%p → installs %p, does not install %p', async (libc, found, notFound) => {
    const rootProjectManifest = {
      dependencies: {
        '@pnpm.e2e/support-different-architectures': '1.0.0',
      },
    }

    prepare(rootProjectManifest)

    await install.handler({
      ...DEFAULT_OPTS,
      rootProjectManifest,
      dir: process.cwd(),
      supportedArchitectures: {
        libc: [libc],
      },
    })

    const pkgDirs = fs.readdirSync(path.resolve('node_modules', '.pnpm'))
    expect(pkgDirs).toContain('@pnpm.e2e+support-different-architectures@1.0.0')
    expect(pkgDirs).toContain(found)
    expect(pkgDirs).not.toContain(notFound)
  })
})

test('install Node.js when devEngines runtime is set with onFail=download', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
        onFail: 'download',
      },
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  project.isExecutable('.bin/node')
  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toStrictEqual({
    node: {
      specifier: 'runtime:24.0.0',
      version: 'runtime:24.0.0',
    },
  })

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-even'])
})

test('do not install Node.js when devEngines runtime is not set to onFail=download', async () => {
  const project = prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '24.0.0',
      },
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].devDependencies).toBeUndefined()
})

test('install restores a deleted pnpm-lock.yaml from the current lockfile without resolution', async () => {
  prepareEmpty()

  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  }, ['is-positive@1.0.0'])

  const originalLockfile = fs.readFileSync('pnpm-lock.yaml', 'utf8')
  rimrafSync('pnpm-lock.yaml')

  // The dead registry proves the repeat install neither resolves nor
  // verifies: the current lockfile (node_modules/.pnpm/lock.yaml) stands in
  // as the wanted lockfile and pnpm-lock.yaml is restored from it.
  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    optimisticRepeatInstall: true,
    registries: { default: 'http://127.0.0.1:9/' },
  })

  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(originalLockfile)
})

test('install --dry-run reports the changes a real install would make, without writing anything', async () => {
  const project = prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  // Add a new dependency so a real install would change the lockfile and node_modules.
  fs.writeFileSync('package.json', JSON.stringify({
    dependencies: { 'is-positive': '1.0.0', 'is-negative': '1.0.0' },
  }))
  const lockfileBefore = fs.readFileSync('pnpm-lock.yaml', 'utf8')

  const output = await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    dryRun: true,
  })

  expect(output).toContain('is-negative')
  // Nothing is written: the lockfile is untouched and the new dependency is not linked.
  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(lockfileBefore)
  project.hasNot('is-negative')
})

test('install --dry-run reports no changes when the project is already up to date', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  const output = await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    dryRun: true,
  })

  expect(output).toContain('up to date')
})

test('install --dry-run reports a specifier-only change to a direct dependency', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  // Change only the specifier; it still resolves to the same version.
  fs.writeFileSync('package.json', JSON.stringify({
    dependencies: { 'is-positive': '~1.0.0' },
  }))
  const lockfileBefore = fs.readFileSync('pnpm-lock.yaml', 'utf8')

  const output = await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    dryRun: true,
  })

  // A real install would rewrite the lockfile's specifier, so this is a change.
  expect(output).not.toContain('up to date')
  expect(output).toContain('is-positive')
  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(lockfileBefore)
})

test('install --dry-run reports a direct dependency moving between groups', async () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
  })

  // Move is-positive from dependencies to devDependencies; the specifier and
  // resolved version are unchanged, but a real install rewrites the importer
  // section of the lockfile.
  fs.writeFileSync('package.json', JSON.stringify({
    devDependencies: { 'is-positive': '1.0.0' },
  }))
  const lockfileBefore = fs.readFileSync('pnpm-lock.yaml', 'utf8')

  const output = await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    dryRun: true,
  })

  expect(output).not.toContain('up to date')
  expect(output).toContain('is-positive')
  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(lockfileBefore)
})

test('install --dry-run reports changes in a workspace without writing', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      dependencies: { 'is-positive': '1.0.0' },
    },
  ])

  const selectWorkspace = () => filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  {
    const { allProjects, selectedProjectsGraph } = await selectWorkspace()
    await install.handler({
      ...DEFAULT_OPTS,
      allProjects,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
      lockfileDir: process.cwd(),
      sharedWorkspaceLockfile: true,
      workspaceDir: process.cwd(),
    })
  }

  // Add a dependency to a workspace project so the shared lockfile is stale.
  fs.writeFileSync('project-1/package.json', JSON.stringify({
    name: 'project-1',
    version: '1.0.0',
    dependencies: { 'is-positive': '1.0.0', 'is-negative': '1.0.0' },
  }))
  const lockfileBefore = fs.readFileSync('pnpm-lock.yaml', 'utf8')
  const projectManifestBefore = fs.readFileSync('project-1/package.json', 'utf8')

  const { allProjects, selectedProjectsGraph } = await selectWorkspace()
  const output = await install.handler({
    ...DEFAULT_OPTS,
    allProjects,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    lockfileDir: process.cwd(),
    sharedWorkspaceLockfile: true,
    workspaceDir: process.cwd(),
    dryRun: true,
  })

  // The recursive path must surface the change rather than mask it as up to date.
  expect(output).not.toContain('up to date')
  expect(output).toContain('is-negative')
  // Nothing is written: not the lockfile, nor the project manifest.
  expect(fs.readFileSync('pnpm-lock.yaml', 'utf8')).toBe(lockfileBefore)
  expect(fs.readFileSync('project-1/package.json', 'utf8')).toBe(projectManifestBefore)
})

test('a config-level dryRun does not turn add into a no-op', async () => {
  prepareEmpty()

  // `--dry-run` is install-only; a config-level `dry-run` (it is a real config
  // key) must not silently make `add` a check-only no-op.
  await add.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    dryRun: true,
  }, ['is-positive@1.0.0'])

  const pkg = loadJsonFileSync<{ dependencies?: Record<string, string> }>(path.resolve('package.json'))
  expect(pkg.dependencies).toStrictEqual({ 'is-positive': '1.0.0' })
})

// Covers https://github.com/pnpm/pnpm/issues/11795
test('repeat install refetches a file: dependency after its contents change', async () => {
  prepareEmpty()

  const localDepDir = path.resolve('..', 'local-dep')
  fs.mkdirSync(localDepDir, { recursive: true })
  fs.writeFileSync(path.join(localDepDir, 'package.json'), JSON.stringify({ name: 'local-dep', version: '1.0.0' }), 'utf8')
  fs.writeFileSync(path.join(localDepDir, 'index.js'), 'v1', 'utf8')
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'project',
    version: '1.0.0',
    dependencies: { 'local-dep': 'file:../local-dep' },
  }), 'utf8')

  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    optimisticRepeatInstall: true,
  })
  expect(fs.readFileSync('node_modules/local-dep/index.js', 'utf8')).toBe('v1')

  // A short delay so the edited file's mtime is unambiguously newer.
  await delay(200)
  fs.writeFileSync(path.join(localDepDir, 'index.js'), 'v2', 'utf8')

  // Without the local-file-deps guard the optimistic fast path would
  // report "Already up to date" here and leave node_modules at v1.
  await install.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    optimisticRepeatInstall: true,
  })
  expect(fs.readFileSync('node_modules/local-dep/index.js', 'utf8')).toBe('v2')
})
