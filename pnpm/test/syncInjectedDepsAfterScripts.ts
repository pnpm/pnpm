import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { type ProjectManifest } from '@pnpm/types'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from './utils'

const f = fixtures(__dirname)

const TEMPLATE_PACKAGE_NAMES = ['foo', 'bar', 'baz'] as const
const TEMPLATE_SCRIPT_NAMES = ['build1.cjs', 'build2.cjs', 'build3.cjs'] as const
const TEMPLATE_FILE_NAMES = [...TEMPLATE_SCRIPT_NAMES, 'should-be-deleted-by-build1.txt', 'should-be-modified-by-build1.txt'] as const

const template = {
  manifests: (): Record<typeof TEMPLATE_PACKAGE_NAMES[number], ProjectManifest> => ({
    foo: {
      name: 'foo',
      version: '0.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
      scripts: {
        build1: 'node ./build1.cjs',
        build2: 'node ./build2.cjs',
        build3: 'node ./build3.cjs',
      },
    },
    bar: {
      name: 'bar',
      version: '0.0.0',
      dependencies: {
        foo: 'workspace:*',
      },
      scripts: {
        build1: 'node ./build1.cjs',
        build2: 'node ./build2.cjs',
        build3: 'node ./build3.cjs',
      },
    },
    baz: {
      name: 'baz',
      version: '0.0.0',
      dependencies: {
        bar: 'workspace:*',
      },
      scripts: {
        build1: 'node ./build1.cjs',
        build2: 'node ./build2.cjs',
        build3: 'node ./build3.cjs',
      },
    },
  }),
  npmrc: (suffix: string[]): string => [
    'reporter=append-only',
    'inject-workspace-packages=true',
    'dedupe-injected-deps=false',
    ...suffix,
  ].join('\n'),
}

function prepareInjectedDepsWorkspace (suffix: string[]) {
  const manifests = template.manifests()
  preparePackages(
    TEMPLATE_PACKAGE_NAMES.map(pkgName => ({
      location: `packages/${pkgName}`,
      package: manifests[pkgName],
    }))
  )

  for (const pkgName of TEMPLATE_PACKAGE_NAMES) {
    f.copy('injected-dep-files', path.join('packages', pkgName))
  }

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const npmrc = template.npmrc(suffix)
  fs.writeFileSync('.npmrc', npmrc)
}

test('with sync-injected-deps-after-scripts', async () => {
  prepareInjectedDepsWorkspace([
    'sync-injected-deps-after-scripts[]=build1',
    'sync-injected-deps-after-scripts[]=build2',
    'sync-injected-deps-after-scripts[]=build3',
  ])

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+packages+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+packages+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')

  // build1 should update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build1'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo')).not.toContain('should-be-deleted-by-build1.txt')
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-added-by-build1.txt', 'utf-8')
    ).toBe(path.resolve('packages/foo/build1.cjs'))
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
    ).toBe('After modification')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar')).not.toContain('should-be-deleted-by-build1.txt')
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-added-by-build1.txt', 'utf-8')
    ).toBe(path.resolve('packages/bar/build1.cjs'))
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
    ).toBe('After modification')
  }

  // build2 should update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build2'])

    // injected foo
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/created-by-build2.txt', 'utf-8')
    ).toBe(path.resolve('packages/foo/build2.cjs'))

    // injected bar
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/created-by-build2.txt', 'utf-8')
    ).toBe(path.resolve('packages/bar/build2.cjs'))
  }
})

test('without sync-injected-deps-after-scripts', async () => {
  prepareInjectedDepsWorkspace([])

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+packages+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+packages+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')

  // build1 should not update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build1'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo')).toContain('should-be-deleted-by-build1.txt')
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo')).not.toContain('should-be-added-by-build1.txt')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar')).toContain('should-be-deleted-by-build1.txt')
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar')).not.toContain('should-be-added-by-build1.txt')
  }

  // build2 should not update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build2'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo')).not.toContain('created-by-build2.txt')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar')).not.toContain('created-by-build2.txt')
  }
})

test('filter scripts', async () => {
  prepareInjectedDepsWorkspace(['sync-injected-deps-after-scripts[]=build1'])

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+packages+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+packages+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')

  // build1 should update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build1'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo')).not.toContain('should-be-deleted-by-build1.txt')
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-added-by-build1.txt', 'utf-8')
    ).toBe(path.resolve('packages/foo/build1.cjs'))
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
    ).toBe('After modification')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar')).not.toContain('should-be-deleted-by-build1.txt')
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-added-by-build1.txt', 'utf-8')
    ).toBe(path.resolve('packages/bar/build1.cjs'))
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
    ).toBe('After modification')
  }

  // build2 should not update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build2'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo')).not.toContain('created-by-build2.txt')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar')).not.toContain('created-by-build2.txt')
  }
})

test('directories and symlinks', async () => {
  prepareInjectedDepsWorkspace([
    'sync-injected-deps-after-scripts[]=build1',
    'sync-injected-deps-after-scripts[]=build2',
    'sync-injected-deps-after-scripts[]=build3',
  ])

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+packages+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+packages+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())

  // build3 should update the injected files
  {
    await execPnpm(['--filter=foo', 'run', 'build3'])

    // should create empty-dirs at source
    expect(fs.readdirSync('packages/foo/empty-dirs/a/a')).toStrictEqual([])
    expect(fs.readdirSync('packages/foo/empty-dirs/a/b')).toStrictEqual([])
    expect(fs.readdirSync('packages/foo/empty-dirs/b/a')).toStrictEqual([])
    expect(fs.readdirSync('packages/foo/empty-dirs/b/b')).toStrictEqual([])

    // should not create empty-dirs at the injected location
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo')).not.toContain('empty-dirs')

    // should recreate a directories tree at the injected location
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo')).toContain('files')
    expect(
      fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/files')
        .sort()
    ).toStrictEqual(['foo', 'foo_bar.txt'])
    expect(
      fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/files/foo')
        .sort()
    ).toStrictEqual(['bar.txt', 'foo'])
    expect(
      fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/files/foo/foo')
        .sort()
    ).toStrictEqual(['foo.txt'])

    // should recreate the structure of the symlinks at the injected location
    // NOTE: The current implementation of @pnpm/directory-fetcher would treat symlinks to dir at real dir
    //       because it uses fs.stat instead of fs.lstat, so testing with fs.realpathSync wouldn't work.
    expect(fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/link-to-a-file', 'utf-8')).toBe('This is foo_bar')
    expect(
      fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/link-to-a-dir')
        .sort()
    ).toStrictEqual(
      fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/files/foo')
        .sort()
    )
  }
})
