import fs from 'fs'
import path from 'path'
import { assertStore } from '@pnpm/assert-store'
import { STORE_VERSION } from '@pnpm/constants'
import { registerProject, getRegisteredProjects } from '@pnpm/package-store'
import { dlx } from '@pnpm/plugin-commands-script-runners'
import { store } from '@pnpm/plugin-commands-store'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { sync as rimraf } from '@zkochan/rimraf'
import { jest } from '@jest/globals'
import execa from 'execa'

const REGISTRY = `http://localhost:${REGISTRY_MOCK_PORT}/`
const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')

const createCacheKey = (...packages: string[]): string => dlx.createCacheKey({
  packages,
  registries: { default: REGISTRY },
})

test('remove unreferenced packages', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [
    pnpmBin,
    'add',
    'is-negative@^2.1.0',
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    `--registry=${REGISTRY}`])
  await execa('node', [
    pnpmBin,
    'remove',
    'is-negative',
    `--store-dir=${storeDir}`,
    `--cache-dir=${cacheDir}`,
    '--config.modules-cache-max-age=0',
  ], { env: { npm_config_registry: REGISTRY } })

  project.storeHas('is-negative', '2.1.0')

  const reporter = jest.fn()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['prune'])

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'info',
      message: 'Removed 1 package',
    })
  )

  project.storeHasNot('is-negative', '2.1.0')

  reporter.mockClear()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['prune'])

  expect(reporter).not.toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'info',
      message: 'Removed 1 package',
    })
  )
  expect(fs.readdirSync(cacheDir)).toStrictEqual([])
})

test('remove packages that are used by project that no longer exist', async () => {
  prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store', STORE_VERSION)
  const { cafsHas, cafsHasNot } = assertStore(storeDir)

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  rimraf('node_modules')

  cafsHas('is-negative', '2.1.0')

  const reporter = jest.fn()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['prune'])

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'info',
      message: 'Removed 1 package',
    })
  )

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'info',
      message: 'Removed 4 files',
    })
  )

  cafsHasNot('is-negative', '2.1.0')
})

test('keep dependencies used by others', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'camelcase-keys@3.0.0', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'add', 'hastscript@3.0.0', '--save-dev', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'remove', 'camelcase-keys', '--store-dir', storeDir], { env: { npm_config_registry: REGISTRY } })

  project.storeHas('camelcase-keys', '3.0.0')
  project.hasNot('camelcase-keys')

  project.storeHas('camelcase', '3.0.0')

  project.storeHas('map-obj', '1.0.1')
  project.hasNot('map-obj')

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['prune'])

  project.storeHasNot('camelcase-keys', '3.0.0')
  project.storeHasNot('map-obj', '1.0.1')
  project.storeHas('camelcase', '3.0.0')
})

test('keep dependency used by package', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'is-not-positive@1.0.0', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY])
  await execa('node', [pnpmBin, 'remove', 'is-not-positive', '--store-dir', storeDir], { env: { npm_config_registry: REGISTRY } })

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['prune'])

  project.storeHas('is-positive', '3.1.0')
})

test('prune will skip scanning non-directory in storeDir', async () => {
  prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')
  await execa('node', [pnpmBin, 'add', 'is-not-positive@1.0.0', 'is-positive@3.1.0', '--store-dir', storeDir, '--registry', REGISTRY])
  fs.writeFileSync(path.join(storeDir, STORE_VERSION, 'files/.DS_store'), 'foobar')

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['prune'])
})

test('prune does not fail if the store contains an unexpected directory', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  project.storeHas('is-negative', '2.1.0')
  const alienDir = path.join(storeDir, STORE_VERSION, 'files/44/directory')
  fs.mkdirSync(alienDir)

  const reporter = jest.fn()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['prune'])

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'warn',
      message: `An alien directory is present in the store: ${alienDir}`,
    })
  )

  // as force is not used, the alien directory is not removed
  expect(fs.existsSync(alienDir)).toBeTruthy()
})

test('prune removes alien files from the store if the --force flag is used', async () => {
  const project = prepare()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  await execa('node', [pnpmBin, 'add', 'is-negative@2.1.0', '--store-dir', storeDir, '--registry', REGISTRY])

  project.storeHas('is-negative', '2.1.0')
  const alienDir = path.join(storeDir, STORE_VERSION, 'files/44/directory')
  fs.mkdirSync(alienDir)

  const reporter = jest.fn()
  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter,
    storeDir,
    userConfig: {},
    force: true,
    dlxCacheMaxAge: Infinity,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['prune'])
  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'warn',
      message: `An alien directory has been removed from the store: ${alienDir}`,
    })
  )
  expect(fs.existsSync(alienDir)).toBeFalsy()
})

describe('prune when store directory is not properly configured', () => {
  test('prune will not fail if the store directory does not exist (ENOENT)', async () => {
    prepareEmpty()
    const nonExistentStoreDir = path.resolve('store')
    const reporter = jest.fn()

    await expect(
      store.handler({
        cacheDir: path.resolve('cache'),
        dir: process.cwd(),
        pnpmHomeDir: '',
        rawConfig: {
          registry: REGISTRY,
        },
        registries: { default: REGISTRY },
        reporter,
        storeDir: nonExistentStoreDir,
        userConfig: {},
        dlxCacheMaxAge: Infinity,
        virtualStoreDirMaxLength: 120,
      }, ['prune'])
    ).resolves.toBeUndefined()

    expect(reporter).toHaveBeenCalledWith(
      expect.objectContaining({
        level: 'info',
        message: 'Removed 0 files',
      })
    )

    expect(reporter).toHaveBeenCalledWith(
      expect.objectContaining({
        level: 'info',
        message: 'Removed 0 packages',
      })
    )
  })

  test('prune will fail for other file-related errors (i.e.; not ENOENT)', async () => {
    prepareEmpty()
    const fileInPlaceOfStoreDir = path.resolve('store')
    fs.writeFileSync(fileInPlaceOfStoreDir, '')
    await expect(
      store.handler({
        cacheDir: path.resolve('cache'),
        dir: process.cwd(),
        pnpmHomeDir: '',
        rawConfig: {
          registry: REGISTRY,
        },
        registries: { default: REGISTRY },
        reporter: jest.fn(),
        storeDir: fileInPlaceOfStoreDir,
        userConfig: {},
        dlxCacheMaxAge: Infinity,
        virtualStoreDirMaxLength: 120,
      }, ['prune'])
    ).rejects.toThrow(/^ENOTDIR/)
  })
})

function createSampleDlxCacheLinkTarget (dirPath: string): void {
  fs.mkdirSync(path.join(dirPath, 'node_modules', '.pnpm'), { recursive: true })
  fs.mkdirSync(path.join(dirPath, 'node_modules', '.bin'), { recursive: true })
  fs.writeFileSync(path.join(dirPath, 'node_modules', '.modules.yaml'), '')
  fs.writeFileSync(path.join(dirPath, 'package.json'), '')
  fs.writeFileSync(path.join(dirPath, 'pnpm-lock.yaml'), '')
}

function createSampleDlxCacheItem (cacheDir: string, cmd: string, now: Date, age: number): void {
  const hash = createCacheKey(cmd)
  const newDate = new Date(now.getTime() - age * 60_000)
  const timeError = 432 // just an arbitrary amount, nothing is special about this number
  const pid = 71014 // just an arbitrary number to represent pid
  const targetName = `${(newDate.getTime() - timeError).toString(16)}-${pid.toString(16)}`
  const linkTarget = path.join(cacheDir, 'dlx', hash, targetName)
  const linkPath = path.join(cacheDir, 'dlx', hash, 'pkg')
  createSampleDlxCacheLinkTarget(linkTarget)
  fs.symlinkSync(linkTarget, linkPath, 'junction')
  fs.lutimesSync(linkPath, newDate, newDate)
}

function createSampleDlxCacheFsTree (cacheDir: string, now: Date, ageTable: Record<string, number>): void {
  for (const [cmd, age] of Object.entries(ageTable)) {
    createSampleDlxCacheItem(cacheDir, cmd, now, age)
  }
}

test('prune removes cache directories that outlives dlx-cache-max-age', async () => {
  prepareEmpty()
  const cacheDir = path.resolve('cache')
  const storeDir = path.resolve('store')

  fs.mkdirSync(path.join(storeDir, STORE_VERSION, 'files'), { recursive: true })
  fs.mkdirSync(path.join(storeDir, STORE_VERSION, 'tmp'), { recursive: true })

  const now = new Date()

  createSampleDlxCacheFsTree(cacheDir, now, {
    foo: 1,
    bar: 5,
    baz: 20,
  })

  await store.handler({
    cacheDir,
    dir: process.cwd(),
    pnpmHomeDir: '',
    rawConfig: {
      registry: REGISTRY,
    },
    registries: { default: REGISTRY },
    reporter () { },
    storeDir,
    userConfig: {},
    dlxCacheMaxAge: 7,
    virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
  }, ['prune'])

  expect(
    fs.readdirSync(path.join(cacheDir, 'dlx'))
      .sort()
  ).toStrictEqual(
    ['foo', 'bar']
      .map(cmd => createCacheKey(cmd))
      .sort()
  )
})

describe('global virtual store prune', () => {
  test('prune cleans up stale project registry entries', async () => {
    const cacheDir = path.resolve('cache')
    const storeDir = path.resolve('store', STORE_VERSION)

    // Setup store directories
    fs.mkdirSync(path.join(storeDir, 'files'), { recursive: true })

    // Create a project and register it
    const projectDir = path.resolve('test-project')
    fs.mkdirSync(projectDir, { recursive: true })
    fs.writeFileSync(path.join(projectDir, 'package.json'), '{}')

    await registerProject(storeDir, projectDir)

    // Verify project is registered
    let projects = await getRegisteredProjects(storeDir)
    expect(projects).toContain(projectDir)

    // Delete the project
    rimraf(projectDir)

    // Run prune - should clean up stale registry entry
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      storeDir,
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['prune'])

    // Verify project was removed from registry
    projects = await getRegisteredProjects(storeDir)
    expect(projects).not.toContain(projectDir)
  })

  test('getRegisteredProjects returns empty array for non-existent registry', async () => {
    const storeDir = path.resolve('new-store', STORE_VERSION)
    const projects = await getRegisteredProjects(storeDir)
    expect(projects).toEqual([])
  })

  test('registerProject creates symlink to project', async () => {
    const storeDir = path.resolve('store2', STORE_VERSION)
    const projectDir = path.resolve('test-project2')

    fs.mkdirSync(projectDir, { recursive: true })
    fs.writeFileSync(path.join(projectDir, 'package.json'), '{}')

    await registerProject(storeDir, projectDir)

    const registryDir = path.join(storeDir, 'projects')
    const entries = fs.readdirSync(registryDir)
    expect(entries).toHaveLength(1)

    const linkPath = path.join(registryDir, entries[0])
    const target = fs.readlinkSync(linkPath)
    expect(path.resolve(path.dirname(linkPath), target)).toBe(projectDir)

    rimraf(projectDir)
  })

  test('prune removes unreferenced packages from global virtual store links directory', async () => {
    const cacheDir = path.resolve('cache')
    const storeDir = path.resolve('store3', STORE_VERSION)

    // Setup store directories
    fs.mkdirSync(path.join(storeDir, 'files'), { recursive: true })

    // Create a fake global virtual store structure
    // Structure: links/{pkgName}/{version}/{hash}/node_modules/{pkgName}/...
    const linksDir = path.join(storeDir, 'links')
    const unreferencedPkg = path.join(linksDir, 'unused-pkg', '1.0.0', 'abc123', 'node_modules', 'unused-pkg')
    const referencedPkg = path.join(linksDir, 'used-pkg', '2.0.0', 'def456', 'node_modules', 'used-pkg')

    fs.mkdirSync(unreferencedPkg, { recursive: true })
    fs.writeFileSync(path.join(unreferencedPkg, 'package.json'), '{}')

    fs.mkdirSync(referencedPkg, { recursive: true })
    fs.writeFileSync(path.join(referencedPkg, 'package.json'), '{}')

    // Create a project that references the 'used-pkg'
    const projectDir = path.resolve('test-project3')
    const projectNodeModules = path.join(projectDir, 'node_modules')
    fs.mkdirSync(projectNodeModules, { recursive: true })
    fs.writeFileSync(path.join(projectDir, 'package.json'), '{}')

    // Create a symlink in the project's node_modules pointing to the referenced package
    fs.symlinkSync(referencedPkg, path.join(projectNodeModules, 'used-pkg'))

    // Register the project
    await registerProject(storeDir, projectDir)

    // Run prune
    const reporter = jest.fn()
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      reporter,
      storeDir,
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['prune'])

    // Verify: unreferenced package should be removed
    expect(fs.existsSync(path.join(linksDir, 'unused-pkg'))).toBe(false)

    // Verify: referenced package should still exist
    expect(fs.existsSync(referencedPkg)).toBe(true)

    rimraf(projectDir)
  })

  test('prune keeps packages that are referenced by multiple projects', async () => {
    const cacheDir = path.resolve('cache')
    const storeDir = path.resolve('store4', STORE_VERSION)

    // Setup store directories
    fs.mkdirSync(path.join(storeDir, 'files'), { recursive: true })

    // Create a package in the global virtual store
    const linksDir = path.join(storeDir, 'links')
    const sharedPkg = path.join(linksDir, 'shared-pkg', '1.0.0', 'hash123', 'node_modules', 'shared-pkg')
    fs.mkdirSync(sharedPkg, { recursive: true })
    fs.writeFileSync(path.join(sharedPkg, 'package.json'), '{}')

    // Create two projects that both reference the same package
    const project1 = path.resolve('test-project-a')
    const project2 = path.resolve('test-project-b')

    fs.mkdirSync(path.join(project1, 'node_modules'), { recursive: true })
    fs.mkdirSync(path.join(project2, 'node_modules'), { recursive: true })
    fs.writeFileSync(path.join(project1, 'package.json'), '{}')
    fs.writeFileSync(path.join(project2, 'package.json'), '{}')

    // Both projects symlink to the shared package
    fs.symlinkSync(sharedPkg, path.join(project1, 'node_modules', 'shared-pkg'))
    fs.symlinkSync(sharedPkg, path.join(project2, 'node_modules', 'shared-pkg'))

    await registerProject(storeDir, project1)
    await registerProject(storeDir, project2)

    // Delete project1 but keep project2
    rimraf(project1)

    // Run prune
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      storeDir,
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['prune'])

    // Package should still exist because project2 references it
    expect(fs.existsSync(sharedPkg)).toBe(true)

    rimraf(project2)
  })

  test('prune removes all packages when no projects reference them', async () => {
    const cacheDir = path.resolve('cache')
    const storeDir = path.resolve('store5', STORE_VERSION)

    // Setup store directories
    fs.mkdirSync(path.join(storeDir, 'files'), { recursive: true })

    // Create packages in the global virtual store with no referencing projects
    const linksDir = path.join(storeDir, 'links')
    const orphanPkg1 = path.join(linksDir, 'orphan-a', '1.0.0', 'hash1', 'node_modules', 'orphan-a')
    const orphanPkg2 = path.join(linksDir, 'orphan-b', '2.0.0', 'hash2', 'node_modules', 'orphan-b')

    fs.mkdirSync(orphanPkg1, { recursive: true })
    fs.mkdirSync(orphanPkg2, { recursive: true })
    fs.writeFileSync(path.join(orphanPkg1, 'package.json'), '{}')
    fs.writeFileSync(path.join(orphanPkg2, 'package.json'), '{}')

    // Create a project that doesn't reference ANY packages in the store
    const projectDir = path.resolve('empty-project')
    fs.mkdirSync(path.join(projectDir, 'node_modules'), { recursive: true })
    fs.writeFileSync(path.join(projectDir, 'package.json'), '{}')

    // Register the project
    await registerProject(storeDir, projectDir)

    const reporter = jest.fn()

    // Run prune - project is registered but references nothing
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      reporter,
      storeDir,
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['prune'])

    // Both orphan packages should be removed since no project references them
    expect(fs.existsSync(path.join(linksDir, 'orphan-a'))).toBe(false)
    expect(fs.existsSync(path.join(linksDir, 'orphan-b'))).toBe(false)

    rimraf(projectDir)
  })

  test('prune preserves transitive dependencies (subdependencies)', async () => {
    const cacheDir = path.resolve('cache')
    const storeDir = path.resolve('store6', STORE_VERSION)

    // Setup store directories
    fs.mkdirSync(path.join(storeDir, 'files'), { recursive: true })

    // Create a realistic package structure with subdependencies:
    // project -> pkg-a -> pkg-b -> pkg-c
    // Also include an unreferenced pkg-d that should be removed
    const linksDir = path.join(storeDir, 'links')

    // pkg-a: root dependency
    const pkgA = path.join(linksDir, 'pkg-a', '1.0.0', 'hash-a', 'node_modules', 'pkg-a')
    const pkgANodeModules = path.join(linksDir, 'pkg-a', '1.0.0', 'hash-a', 'node_modules')

    // pkg-b: subdependency of pkg-a
    const pkgB = path.join(linksDir, 'pkg-b', '2.0.0', 'hash-b', 'node_modules', 'pkg-b')
    const pkgBNodeModules = path.join(linksDir, 'pkg-b', '2.0.0', 'hash-b', 'node_modules')

    // pkg-c: subdependency of pkg-b
    const pkgC = path.join(linksDir, 'pkg-c', '3.0.0', 'hash-c', 'node_modules', 'pkg-c')

    // pkg-d: unreferenced package
    const pkgD = path.join(linksDir, 'pkg-d', '4.0.0', 'hash-d', 'node_modules', 'pkg-d')

    // Create all packages
    fs.mkdirSync(pkgA, { recursive: true })
    fs.mkdirSync(pkgB, { recursive: true })
    fs.mkdirSync(pkgC, { recursive: true })
    fs.mkdirSync(pkgD, { recursive: true })
    fs.writeFileSync(path.join(pkgA, 'package.json'), '{"name": "pkg-a"}')
    fs.writeFileSync(path.join(pkgB, 'package.json'), '{"name": "pkg-b"}')
    fs.writeFileSync(path.join(pkgC, 'package.json'), '{"name": "pkg-c"}')
    fs.writeFileSync(path.join(pkgD, 'package.json'), '{"name": "pkg-d"}')

    // Create symlinks for subdependencies within the store:
    // pkg-a/node_modules/pkg-b -> pkg-b
    fs.symlinkSync(pkgB, path.join(pkgANodeModules, 'pkg-b'))
    // pkg-b/node_modules/pkg-c -> pkg-c
    fs.symlinkSync(pkgC, path.join(pkgBNodeModules, 'pkg-c'))

    // Create a project that references pkg-a
    const projectDir = path.resolve('complex-project')
    const projectNodeModules = path.join(projectDir, 'node_modules')
    fs.mkdirSync(projectNodeModules, { recursive: true })
    fs.writeFileSync(path.join(projectDir, 'package.json'), '{}')

    // Project references pkg-a
    fs.symlinkSync(pkgA, path.join(projectNodeModules, 'pkg-a'))

    await registerProject(storeDir, projectDir)

    const reporter = jest.fn()

    // Run prune
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      reporter,
      storeDir,
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['prune'])

    // Verify: all transitive dependencies should be preserved
    expect(fs.existsSync(pkgA)).toBe(true)
    expect(fs.existsSync(pkgB)).toBe(true)
    expect(fs.existsSync(pkgC)).toBe(true)

    // Verify: unreferenced pkg-d should be removed
    expect(fs.existsSync(path.join(linksDir, 'pkg-d'))).toBe(false)

    rimraf(projectDir)
  })
})
