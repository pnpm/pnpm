import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from './utils'

const f = fixtures(__dirname)

const PKG_FILES = [
  ...fs.readdirSync(f.find('injected-dep-files')),
  'package.json',
].sort()

function prepareInjectedDepsWorkspace (syncInjectedDepsAfterScripts: string[]) {
  const scripts = {
    build1: 'node ./build1.cjs',
    build2: 'node ./build2.cjs',
    build3: 'node ./build3.cjs',
  }
  preparePackages([
    {
      name: 'foo',
      version: '0.0.0',
      dependencies: {
        'is-positive': '1.0.0',
      },
      scripts,
    },
    {
      name: 'bar',
      version: '0.0.0',
      dependencies: {
        foo: 'workspace:*',
      },
      scripts,
    },
    {
      name: 'baz',
      version: '0.0.0',
      dependencies: {
        bar: 'workspace:*',
      },
      scripts,
    },
  ])

  for (const pkgName of ['foo', 'bar', 'baz']) {
    f.copy('injected-dep-files', pkgName)
  }

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['*'],
  })

  fs.writeFileSync('.npmrc', [
    'reporter=append-only',
    'inject-workspace-packages=true',
    'dedupe-injected-deps=false',
    ...syncInjectedDepsAfterScripts.map((scriptName) => `sync-injected-deps-after-scripts[]=${scriptName}`),
  ].join('\n'))
}

test('with sync-injected-deps-after-scripts', async () => {
  prepareInjectedDepsWorkspace(['build1', 'build2', 'build3'])

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo').sort()).toStrictEqual(PKG_FILES)
  expect(
    fs.readFileSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar').sort()).toStrictEqual(PKG_FILES)
  expect(
    fs.readFileSync('node_modules/.pnpm/bar@file+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')

  // build1 should update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build1'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo')).not.toContain('should-be-deleted-by-build1.txt')
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/should-be-added-by-build1.txt', 'utf-8')
    ).toBe(path.resolve('foo/build1.cjs'))
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
    ).toBe('After modification')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar')).not.toContain('should-be-deleted-by-build1.txt')
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+bar/node_modules/bar/should-be-added-by-build1.txt', 'utf-8')
    ).toBe(path.resolve('bar/build1.cjs'))
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
    ).toBe('After modification')
  }

  // build2 should update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build2'])

    // injected foo
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/created-by-build2.txt', 'utf-8')
    ).toBe(path.resolve('foo/build2.cjs'))

    // injected bar
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+bar/node_modules/bar/created-by-build2.txt', 'utf-8')
    ).toBe(path.resolve('bar/build2.cjs'))
  }
})

test('without sync-injected-deps-after-scripts', async () => {
  prepareInjectedDepsWorkspace([])

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo').sort()).toStrictEqual(PKG_FILES)
  expect(
    fs.readFileSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar').sort()).toStrictEqual(PKG_FILES)
  expect(
    fs.readFileSync('node_modules/.pnpm/bar@file+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')

  // build1 should not update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build1'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo')).toContain('should-be-deleted-by-build1.txt')
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo')).not.toContain('should-be-added-by-build1.txt')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar')).toContain('should-be-deleted-by-build1.txt')
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar')).not.toContain('should-be-added-by-build1.txt')
  }

  // build2 should not update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build2'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo')).not.toContain('created-by-build2.txt')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar')).not.toContain('created-by-build2.txt')
  }
})

test('filter scripts', async () => {
  prepareInjectedDepsWorkspace(['build1'])

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo').sort()).toStrictEqual(PKG_FILES)
  expect(
    fs.readFileSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar').sort()).toStrictEqual(PKG_FILES)
  expect(
    fs.readFileSync('node_modules/.pnpm/bar@file+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification\n')

  // build1 should update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build1'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo')).not.toContain('should-be-deleted-by-build1.txt')
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/should-be-added-by-build1.txt', 'utf-8')
    ).toBe(path.resolve('foo/build1.cjs'))
    expect(
      fs.readFileSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
    ).toBe('After modification')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar')).not.toContain('should-be-deleted-by-build1.txt')
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+bar/node_modules/bar/should-be-added-by-build1.txt', 'utf-8')
    ).toBe(path.resolve('bar/build1.cjs'))
    expect(
      fs.readFileSync('node_modules/.pnpm/bar@file+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
    ).toBe('After modification')
  }

  // build2 should not update the injected files
  {
    await execPnpm(['--recursive', 'run', 'build2'])

    // injected foo
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo')).not.toContain('created-by-build2.txt')

    // injected bar
    expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar')).not.toContain('created-by-build2.txt')
  }
})

test('directories and symlinks', async () => {
  prepareInjectedDepsWorkspace(['build1', 'build2', 'build3'])

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo').sort()).toStrictEqual(PKG_FILES)
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+bar/node_modules/bar').sort()).toStrictEqual(PKG_FILES)

  // build3 should update the injected files
  {
    await execPnpm(['--filter=foo', 'run', 'build3'])

    // should create empty-dirs at source
    expect(fs.readdirSync('foo/empty-dirs/a/a')).toStrictEqual([])
    expect(fs.readdirSync('foo/empty-dirs/a/b')).toStrictEqual([])
    expect(fs.readdirSync('foo/empty-dirs/b/a')).toStrictEqual([])
    expect(fs.readdirSync('foo/empty-dirs/b/b')).toStrictEqual([])

    // should not create empty-dirs at the injected location
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo')).not.toContain('empty-dirs')

    // should recreate a directories tree at the injected location
    expect(fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo')).toContain('files')
    expect(
      fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/files')
        .sort()
    ).toStrictEqual(['foo', 'foo_bar.txt'])
    expect(
      fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/files/foo')
        .sort()
    ).toStrictEqual(['bar.txt', 'foo'])
    expect(
      fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/files/foo/foo')
        .sort()
    ).toStrictEqual(['foo.txt'])

    // should recreate the structure of the symlinks at the injected location
    // NOTE: The current implementation of @pnpm/directory-fetcher would treat symlinks to dir at real dir
    //       because it uses fs.stat instead of fs.lstat, so testing with fs.realpathSync wouldn't work.
    expect(fs.readFileSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/link-to-a-file', 'utf-8')).toBe('This is foo_bar')
    expect(
      fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/link-to-a-dir')
        .sort()
    ).toStrictEqual(
      fs.readdirSync('node_modules/.pnpm/foo@file+foo/node_modules/foo/files/foo')
        .sort()
    )
  }
})
