import fs from 'fs'
import path from 'path'
import { sync as writeYamlFile } from 'write-yaml-file'
import { type Config } from '@pnpm/config'
import { prepare } from '@pnpm/prepare'
import { execPnpmSync } from '../utils/index.js'

test('pnpm config list reads npm options but ignores other settings from .npmrc', () => {
  prepare()
  fs.writeFileSync('.npmrc', [
    // npm options
    '//my-org.registry.example.com:username=some-employee',
    '//my-org.registry.example.com:_authToken=some-employee-token',
    '@my-org:registry=https://my-org.registry.example.com',
    '@jsr:registry=https://not-actually-jsr.example.com',
    'username=example-user-name',
    '_authToken=example-auth-token',

    // pnpm options
    'dlx-cache-max-age=1234',
    'only-built-dependencies[]=foo',
    'only-built-dependencies[]=bar',
    'packages[]=baz',
    'packages[]=qux',
  ].join('\n'))

  const { stdout } = execPnpmSync(['config', 'list', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).toMatchObject({
    '//my-org.registry.example.com:username': '(protected)',
    '//my-org.registry.example.com:_authToken': '(protected)',
    '@my-org:registry': 'https://my-org.registry.example.com',
    '@jsr:registry': 'https://not-actually-jsr.example.com',
  } as Partial<Config>)
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlx-cache-max-age'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlxCacheMaxAge'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['only-built-dependencies'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['onlyBuiltDependencies'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['packages'])
})

test('pnpm config list reads workspace-specific settings from pnpm-workspace.yaml', () => {
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

  const { stdout } = execPnpmSync(['config', 'list'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).toStrictEqual(expect.objectContaining(workspaceManifest))
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlx-cache-max-age'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['only-built-dependencies'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['package-extensions'])
})

test('pnpm config list shows settings from global config.yaml', () => {
  prepare()

  const XDG_CONFIG_HOME = path.resolve('.config')
  const configDir = path.join(XDG_CONFIG_HOME, 'pnpm')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFile(path.join(configDir, 'config.yaml'), {
    dangerouslyAllowAllBuilds: true,
    dlxCacheMaxAge: 1234,
    dev: true,
    frozenLockfile: true,
    catalog: {
      react: '^19.0.0',
    },
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
  })

  const { stdout } = execPnpmSync(['config', 'list'], {
    expectSuccess: true,
    env: {
      XDG_CONFIG_HOME,
    },
  })
  expect(JSON.parse(stdout.toString())).toStrictEqual(expect.objectContaining({
    dangerouslyAllowAllBuilds: true,
    dlxCacheMaxAge: 1234,
  }))

  // doesn't list CLI options
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dev'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['frozenLockfile'])

  // doesn't list workspace-specific settings
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['catalog'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['catalogs'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['packages'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['packageExtensions'])

  // doesn't list the kebab-case versions
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['frozen-lockfile'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['only-built-dependencies'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['dlx-cache-max-age'])
  expect(JSON.parse(stdout.toString())).not.toHaveProperty(['package-extensions'])
})
