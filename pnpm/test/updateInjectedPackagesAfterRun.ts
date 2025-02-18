import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from './utils'

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
  files: (): Record<typeof TEMPLATE_FILE_NAMES[number], string> => ({
    'build1.cjs': `
      const fs = require('fs')
      fs.rmSync('should-be-deleted-by-build1.txt', { force: true })
      fs.writeFileSync('should-be-modified-by-build1.txt', 'After modification')
      fs.writeFileSync('should-be-added-by-build1.txt', __filename)
    `,
    'build2.cjs': `
      const fs = require('fs')
      fs.writeFileSync('created-by-build2.txt', __filename)
    `,
    'build3.cjs': `
      const fs = require('fs')
      console.log('Creating a tree of empty directories...')
      fs.mkdirSync('empty-dirs/a/a/', { recursive: true })
      fs.mkdirSync('empty-dirs/a/b/', { recursive: true })
      fs.mkdirSync('empty-dirs/b/a/', { recursive: true })
      fs.mkdirSync('empty-dirs/b/b/', { recursive: true })
      console.log('Creating a tree of real files...')
      fs.mkdirSync('files/foo/foo/', { recursive: true })
      fs.writeFileSync('files/foo/foo/foo.txt', '')
      fs.writeFileSync('files/foo/bar.txt', '')
      fs.writeFileSync('files/foo_bar.txt', 'This is foo_bar')
      console.log('Creating symlinks...')
      fs.symlinkSync('files/foo_bar.txt', 'link-to-a-file', 'file')
      fs.symlinkSync('files/foo', 'link-to-a-dir', 'dir')
    `,
    'should-be-deleted-by-build1.txt': '',
    'should-be-modified-by-build1.txt': 'Before modification',
  }),
  npmrc: (...suffix: string[]): string => [
    'reporter=append-only',
    'inject-workspace-packages=true',
    'dedupe-injected-deps=false',
    ...suffix,
  ].join('\n'),
}

test('update-injected-packages-after-run=true', async () => {
  const manifests = template.manifests()
  preparePackages(
    TEMPLATE_PACKAGE_NAMES.map(pkgName => ({
      location: `packages/${pkgName}`,
      package: manifests[pkgName],
    }))
  )

  const files = template.files()
  for (const pkgName of TEMPLATE_PACKAGE_NAMES) {
    for (const fileName of TEMPLATE_FILE_NAMES) {
      fs.writeFileSync(path.join('packages', pkgName, fileName), files[fileName])
    }
  }

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const npmrc = template.npmrc('update-injected-packages-after-run=true')
  fs.writeFileSync('.npmrc', npmrc)

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+packages+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+packages+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification')
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification')

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

test('update-injected-packages-after-run=false', async () => {
  const manifests = template.manifests()
  preparePackages(
    TEMPLATE_PACKAGE_NAMES.map(pkgName => ({
      location: `packages/${pkgName}`,
      package: manifests[pkgName],
    }))
  )

  const files = template.files()
  for (const pkgName of TEMPLATE_PACKAGE_NAMES) {
    for (const fileName of TEMPLATE_FILE_NAMES) {
      fs.writeFileSync(path.join('packages', pkgName, fileName), files[fileName])
    }
  }

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const npmrc = template.npmrc('update-injected-packages-after-run=false')
  fs.writeFileSync('.npmrc', npmrc)

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+packages+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+packages+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification')
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification')

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
  const manifests = template.manifests()
  preparePackages(
    TEMPLATE_PACKAGE_NAMES.map(pkgName => ({
      location: `packages/${pkgName}`,
      package: manifests[pkgName],
    }))
  )

  const files = template.files()
  for (const pkgName of TEMPLATE_PACKAGE_NAMES) {
    for (const fileName of TEMPLATE_FILE_NAMES) {
      fs.writeFileSync(path.join('packages', pkgName, fileName), files[fileName])
    }
  }

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const npmrc = template.npmrc('update-injected-packages-after-run[]=build1')
  fs.writeFileSync('.npmrc', npmrc)

  await execPnpm(['install'])
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('foo@file+packages+foo')
  expect(fs.readdirSync('node_modules/.pnpm')).toContain('bar@file+packages+bar')
  expect(fs.readdirSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/foo@file+packages+foo/node_modules/foo/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification')
  expect(fs.readdirSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar').sort()).toStrictEqual([
    ...TEMPLATE_FILE_NAMES,
    'package.json',
  ].sort())
  expect(
    fs.readFileSync('node_modules/.pnpm/bar@file+packages+bar/node_modules/bar/should-be-modified-by-build1.txt', 'utf-8')
  ).toBe('Before modification')

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
  const manifests = template.manifests()
  preparePackages(
    TEMPLATE_PACKAGE_NAMES.map(pkgName => ({
      location: `packages/${pkgName}`,
      package: manifests[pkgName],
    }))
  )

  const files = template.files()
  for (const pkgName of TEMPLATE_PACKAGE_NAMES) {
    for (const fileName of TEMPLATE_FILE_NAMES) {
      fs.writeFileSync(path.join('packages', pkgName, fileName), files[fileName])
    }
  }

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const npmrc = template.npmrc('update-injected-packages-after-run=true')
  fs.writeFileSync('.npmrc', npmrc)

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
