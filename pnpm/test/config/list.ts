import fs from 'fs'
import * as ini from 'ini'
import { sync as writeYamlFile } from 'write-yaml-file'
import { type Config } from '@pnpm/config'
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
    dlxCacheMaxAge: 1234,
    onlyBuiltDependencies: ['foo', 'bar'],
  } as Partial<Config>)
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

test('pnpm config list ignores unknown kebab-case options from .npmrc', () => {
  prepare()
  fs.writeFileSync('.npmrc', [
    'this-option-is-not-defined-by-pnpm=some-value',
  ].join('\n'))

  {
    const { stdout } = execPnpmSync(['config', 'list'], { expectSuccess: true })
    expect(ini.decode(stdout.toString())).not.toHaveProperty(['this-option-is-not-defined-by-pnpm'])
    expect(ini.decode(stdout.toString())).not.toHaveProperty(['thisOptionIsNotDefinedByPnpm'])
  }

  {
    const { stdout } = execPnpmSync(['config', 'list', '--json'], { expectSuccess: true })
    expect(ini.decode(stdout.toString())).not.toHaveProperty(['this-option-is-not-defined-by-pnpm'])
    expect(ini.decode(stdout.toString())).not.toHaveProperty(['thisOptionIsNotDefinedByPnpm'])
  }
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
  expect(JSON.parse(stdout.toString())).toStrictEqual(expect.objectContaining(workspaceManifest))
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlx-cache-max-age'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['only-built-dependencies'])
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

// This behavior is not really desired, it is but a side-effect of the config loader not validating pnpm-workspace.yaml.
// Still, removing it can be considered a breaking change, so this test is here to track for that.
test('pnpm config list still reads unknown camelCase keys from pnpm-workspace.yaml', () => {
  const workspaceManifest = {
    thisOptionIsNotDefinedByPnpm: 'some-value',
  }

  prepare()
  writeYamlFile('pnpm-workspace.yaml', workspaceManifest)

  {
    const { stdout } = execPnpmSync(['config', 'list'], { expectSuccess: true })
    expect(ini.decode(stdout.toString())).toMatchObject(workspaceManifest)
    expect(ini.decode(stdout.toString())).not.toHaveProperty(['this-option-is-not-defined-by-pnpm'])
  }

  {
    const { stdout } = execPnpmSync(['config', 'list', '--json'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toMatchObject(workspaceManifest)
    expect(JSON.parse(stdout.toString())).not.toHaveProperty(['this-option-is-not-defined-by-pnpm'])
  }
})

test('pnpm config list --json shows all keys in camelCase', () => {
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
  expect(JSON.parse(stdout.toString())).toStrictEqual(expect.objectContaining(workspaceManifest))
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlx-cache-max-age'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['only-built-dependencies'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['package-extensions'])
})

test('pnpm config list without --json shows rc options in kebab-case and workspace-specific settings in camelCase', () => {
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

  const { stdout } = execPnpmSync(['config', 'list'], { expectSuccess: true })
  expect(ini.decode(stdout.toString())).toEqual(expect.objectContaining({
    'dlx-cache-max-age': String(workspaceManifest.dlxCacheMaxAge), // must be a string because ini doesn't decode to numbers
    'only-built-dependencies': workspaceManifest.onlyBuiltDependencies,
    packages: workspaceManifest.packages,
    packageExtensions: workspaceManifest.packageExtensions,
  }))
  expect(ini.decode(stdout.toString())).not.toHaveProperty(['dlxCacheMaxAge'])
  expect(ini.decode(stdout.toString())).not.toHaveProperty(['onlyBuiltDependencies'])
  expect(ini.decode(stdout.toString())).not.toHaveProperty(['package-extensions'])
})
