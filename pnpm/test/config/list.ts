import fs from 'fs'
import { sync as writeYamlFile } from 'write-yaml-file'
import { prepare } from '@pnpm/prepare'
import { execPnpmSync } from '../utils/index.js'

test('pnpm config list reads rc options but ignores workspace-specific settings from .npmrc', () => {
  prepare()
  fs.writeFileSync('.npmrc', [
    // rc options
    'dlx-cache-max-age=1234',
    'only-built-dependencies[]=foo',
    'only-built-dependencies[]=bar',

    // workspace-specific settings
    'packages[]=baz',
    'packages[]=qux',
  ].join('\n'))

  const { stdout } = execPnpmSync(['config', 'list', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).toMatchObject({
    'dlx-cache-max-age': 1234,
    'only-built-dependencies': ['foo', 'bar'],
  })
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['packages'])
})

test('pnpm config list ignores non kebab-case options from .npmrc', () => {
  prepare()
  fs.writeFileSync('.npmrc', [
    'dlxCacheMaxAge=1234',
    'onlyBuiltDependencies[]=foo',
    'onlyBuiltDependencies[]=bar',
  ].join('\n'))

  const { stdout } = execPnpmSync(['config', 'list', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlx-cache-max-age'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlxCacheMaxAge'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['only-built-dependencies'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['onlyBuiltDependencies'])
})

test('pnpm config list reads both rc options and workspace-specific settings from pnpm-workspace.yaml', () => {
  const workspaceManifest = {
    dlxCacheMaxAge: 1234,
    onlyBuiltDependencies: ['foo', 'bar'],
    packages: ['baz', 'qux'],
    packageExtensions: {
      '@babel/parser': {
        peerDependencies: {
          '@babel/types': '*',
        },
      },
      'jest-circus': {
        dependencies: {
          slash: '3',
        },
      },
    },
  }

  prepare()
  writeYamlFile('pnpm-workspace.yaml', workspaceManifest)

  const { stdout } = execPnpmSync(['config', 'list', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).toStrictEqual(expect.objectContaining({
    'dlx-cache-max-age': workspaceManifest.dlxCacheMaxAge,
    'only-built-dependencies': workspaceManifest.onlyBuiltDependencies,
    packages: workspaceManifest.packages,
    packageExtensions: workspaceManifest.packageExtensions,
  }))
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlxCacheMaxAge'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['onlyBuiltDependencies'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['package-extensions'])
})

test('pnpm config list ignores non camelCase settings from pnpm-workspace.yaml', () => {
  const workspaceManifest = {
    'dlx-cache-max-age': 1234,
    'only-built-dependencies': ['foo', 'bar'],
    'package-extensions': {
      '@babel/parser': {
        peerDependencies: {
          '@babel/types': '*',
        },
      },
      'jest-circus': {
        dependencies: {
          slash: '3',
        },
      },
    },
  }

  prepare()
  writeYamlFile('pnpm-workspace.yaml', workspaceManifest)

  const { stdout } = execPnpmSync(['config', 'list', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlx-cache-max-age'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlxCacheMaxAge'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['only-built-dependencies'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['onlyBuiltDependencies'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['package-extensions'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['packageExtensions'])
})
