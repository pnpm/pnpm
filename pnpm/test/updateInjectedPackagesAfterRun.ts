import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { type ProjectManifest } from '@pnpm/types'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from './utils'

const TEMPLATE_PACKAGE_NAMES = ['foo', 'bar', 'baz'] as const
const TEMPLATE_FILE_NAMES = ['build1.cjs', 'build2.cjs', 'should-be-deleted-by-build1.txt', 'should-be-modified-by-build1.txt'] as const

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

test('update-injected-files-after-run=true', async () => {
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

  const npmrc = template.npmrc('update-injected-files-after-run=true')
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

test('update-injected-files-after-run=false', async () => {
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

  const npmrc = template.npmrc('update-injected-files-after-run=false')
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

  const npmrc = template.npmrc('update-injected-files-after-run[]=build1')
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
