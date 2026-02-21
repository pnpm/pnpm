import fs from 'fs'
import path from 'path'
import { assertStore } from '@pnpm/assert-store'
import { STORE_VERSION } from '@pnpm/constants'
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
  project.storeHasNot('is-negative', '2.1.0')
  project.storeHas('totally-not-is-negative', '999.2.1.0')
  project.storeHasNot('totally-not-is-negative', '999.2.1.0')

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
    reporter () {},
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
  test('prune removes unreferenced packages from global virtual store', async () => {
    // Create project that installs a package with global virtual store enabled
    prepare({
      dependencies: {
        'is-positive': '1.0.0',
      },
    })
    // Store should be OUTSIDE the project directory to ensure proper project registration
    const cacheDir = path.resolve('..', 'cache')
    const storeDir = path.resolve('..', 'store')

    // Install with global virtual store enabled
    await execa('node', [
      pnpmBin,
      'install',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      `--registry=${REGISTRY}`,
      '--config.enableGlobalVirtualStore=true',
      '--config.ci=false', // This is needed because enableGlobalVirtualStore is set to fails in CI
    ])

    // Verify the links directory was created
    const linksDir = path.join(storeDir, STORE_VERSION, 'links')
    expect(fs.existsSync(linksDir)).toBe(true)

    // Remove the dependency from package.json and reinstall
    fs.writeFileSync('package.json', JSON.stringify({ dependencies: {} }))
    await execa('node', [
      pnpmBin,
      'install',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      `--registry=${REGISTRY}`,
      '--config.enableGlobalVirtualStore=true',
      '--config.ci=false',
    ])

    // Run prune - should remove the now-unreferenced package
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      storeDir: path.join(storeDir, STORE_VERSION),
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['prune'])

    // Verify: is-positive should no longer exist in links/@/ directory
    const unscopedDir = path.join(linksDir, '@')
    const entries = fs.existsSync(unscopedDir) ? fs.readdirSync(unscopedDir) : []
    expect(entries).not.toContain('is-positive')
  })

  test('prune keeps packages that are referenced by multiple projects', async () => {
    const storeDir = path.resolve('shared-store')
    const cacheDir = path.resolve('cache')

    // Create first project with is-positive
    const project1Dir = path.resolve('project1')
    fs.mkdirSync(project1Dir, { recursive: true })
    fs.writeFileSync(path.join(project1Dir, 'package.json'), JSON.stringify({
      dependencies: { 'is-positive': '1.0.0' },
    }))

    await execa('node', [
      pnpmBin,
      'install',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      `--registry=${REGISTRY}`,
      '--config.enableGlobalVirtualStore=true',
      '--config.ci=false',
    ], { cwd: project1Dir })

    // Create second project with the same dependency
    const project2Dir = path.resolve('project2')
    fs.mkdirSync(project2Dir, { recursive: true })
    fs.writeFileSync(path.join(project2Dir, 'package.json'), JSON.stringify({
      dependencies: { 'is-positive': '1.0.0' },
    }))

    await execa('node', [
      pnpmBin,
      'install',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      `--registry=${REGISTRY}`,
      '--config.enableGlobalVirtualStore=true',
      '--config.ci=false',
    ], { cwd: project2Dir })

    // Delete project1
    rimraf(project1Dir)

    // Verify package still exists in links/@/ directory
    const linksDir = path.join(storeDir, STORE_VERSION, 'links')
    const unscopedDir = path.join(linksDir, '@')
    const beforePrune = fs.readdirSync(unscopedDir)
    expect(beforePrune).toContain('is-positive')

    // Run prune
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      storeDir: path.join(storeDir, STORE_VERSION),
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['prune'])

    // Package should still exist because project2 references it
    const afterPrune = fs.readdirSync(unscopedDir)
    expect(afterPrune).toContain('is-positive')

    rimraf(project2Dir)
  })

  test('prune removes packages when project using them is deleted', async () => {
    const storeDir = path.resolve('orphan-store')
    const cacheDir = path.resolve('cache')

    // Create first project with is-positive
    const project1Dir = path.resolve('orphan-project1')
    fs.mkdirSync(project1Dir, { recursive: true })
    fs.writeFileSync(path.join(project1Dir, 'package.json'), JSON.stringify({
      dependencies: { 'is-positive': '1.0.0' },
    }))

    await execa('node', [
      pnpmBin,
      'install',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      `--registry=${REGISTRY}`,
      '--config.enableGlobalVirtualStore=true',
      '--config.ci=false',
    ], { cwd: project1Dir })

    // Create second project with a different package (so it stays)
    const project2Dir = path.resolve('orphan-project2')
    fs.mkdirSync(project2Dir, { recursive: true })
    fs.writeFileSync(path.join(project2Dir, 'package.json'), JSON.stringify({
      dependencies: { 'is-negative': '1.0.0' },
    }))

    await execa('node', [
      pnpmBin,
      'install',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      `--registry=${REGISTRY}`,
      '--config.enableGlobalVirtualStore=true',
      '--config.ci=false',
    ], { cwd: project2Dir })

    // Verify both packages exist in links/@/ directory
    const linksDir = path.join(storeDir, STORE_VERSION, 'links')
    const unscopedDir = path.join(linksDir, '@')
    expect(fs.existsSync(unscopedDir)).toBe(true)
    const beforePrune = fs.readdirSync(unscopedDir)
    expect(beforePrune).toContain('is-positive')
    expect(beforePrune).toContain('is-negative')

    // Delete project1 (which uses is-positive)
    rimraf(project1Dir)

    // Run prune
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      storeDir: path.join(storeDir, STORE_VERSION),
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['prune'])

    // is-positive should be removed since project1 was deleted
    const afterPrune = fs.readdirSync(unscopedDir)
    expect(afterPrune).not.toContain('is-positive')
    // is-negative should remain since project2 still exists
    expect(afterPrune).toContain('is-negative')

    rimraf(project2Dir)
  })

  test('prune preserves transitive dependencies and removes isolated ones', async () => {
    // Create project with three packages:
    // - @pnpm.e2e/pkg-with-1-dep has transitive dep @pnpm.e2e/dep-of-pkg-with-1-dep
    // - @pnpm.e2e/romeo has transitive dep @pnpm.e2e/romeo-dep
    // - is-positive has no transitive deps
    prepare({
      dependencies: {
        '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
        '@pnpm.e2e/romeo': '1.0.0',
        'is-positive': '1.0.0',
      },
    })

    // Store should be OUTSIDE the project directory to avoid findAllNodeModulesDirs
    // scanning the store's internal node_modules
    const storeDir = path.resolve('..', 'transitive-store')
    const cacheDir = path.resolve('..', 'cache')

    await execa('node', [
      pnpmBin,
      'install',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      `--registry=${REGISTRY}`,
      '--config.enableGlobalVirtualStore=true',
      '--config.ci=false',
    ])

    // Verify all packages exist in links directory
    const linksDir = path.join(storeDir, STORE_VERSION, 'links')

    // Scoped packages are in links/@pnpm.e2e/pkg-name/
    const scopeDir = path.join(linksDir, '@pnpm.e2e')
    const scopedPkgs = fs.readdirSync(scopeDir)
    expect(scopedPkgs).toContain('pkg-with-1-dep')
    expect(scopedPkgs).toContain('dep-of-pkg-with-1-dep')
    expect(scopedPkgs).toContain('romeo')
    expect(scopedPkgs).toContain('romeo-dep')
    // Unscoped packages are in links/@/pkg-name/ (uniform 4-level depth)
    const unscopedDir = path.join(linksDir, '@')
    const unscopedPkgs = fs.readdirSync(unscopedDir)
    expect(unscopedPkgs).toContain('is-positive')

    // Remove @pnpm.e2e/pkg-with-1-dep, keeping romeo and is-positive
    fs.writeFileSync('package.json', JSON.stringify({
      dependencies: {
        '@pnpm.e2e/romeo': '1.0.0',
        'is-positive': '1.0.0',
      },
    }))
    await execa('node', [
      pnpmBin,
      'install',
      `--store-dir=${storeDir}`,
      `--cache-dir=${cacheDir}`,
      `--registry=${REGISTRY}`,
      '--config.enableGlobalVirtualStore=true',
      '--config.ci=false',
    ])

    // Run prune
    await store.handler({
      cacheDir,
      dir: process.cwd(),
      pnpmHomeDir: '',
      rawConfig: {
        registry: REGISTRY,
      },
      registries: { default: REGISTRY },
      storeDir: path.join(storeDir, STORE_VERSION),
      userConfig: {},
      dlxCacheMaxAge: Infinity,
      virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
    }, ['prune'])

    // Verify:
    // - pkg-with-1-dep and its transitive dep-of-pkg-with-1-dep should be removed
    // - romeo and its transitive romeo-dep should still exist
    // - is-positive should still exist
    const afterPruneScopes = fs.readdirSync(linksDir)
    expect(afterPruneScopes).toContain('@') // unscoped packages scope
    const unscopedAfterPrune = fs.readdirSync(unscopedDir)
    expect(unscopedAfterPrune).toContain('is-positive')

    const scopedPkgsAfter = fs.readdirSync(scopeDir)
    // pkg-with-1-dep and its transitive dep should be removed
    expect(scopedPkgsAfter).not.toEqual(expect.arrayContaining([expect.stringContaining('pkg-with-1-dep')]))
    expect(scopedPkgsAfter).not.toEqual(expect.arrayContaining([expect.stringContaining('dep-of-pkg-with-1-dep')]))
    // romeo and its transitive dep should be preserved
    expect(scopedPkgsAfter).toContain('romeo')
    expect(scopedPkgsAfter).toContain('romeo-dep')
  })
})
