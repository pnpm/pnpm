import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import PATH_NAME from 'path-name'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

test('pnpm config get reads npm options but ignores other settings from .npmrc', () => {
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
    'trust-policy-exclude[]=foo',
    'trust-policy-exclude[]=bar',
    'packages[]=baz',
    'packages[]=qux',
  ].join('\n'))

  // `config get @<scope>:registry` reports the merged (normalized) URL —
  // the same one `pnpm publish` and the resolvers use — see pnpm/pnpm#11492.
  {
    const { stdout } = execPnpmSync(['config', 'get', '@my-org:registry'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('https://my-org.registry.example.com/')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '@jsr:registry'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('https://not-actually-jsr.example.com/')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', 'dlx-cache-max-age'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('undefined')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', 'dlxCacheMaxAge'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('undefined')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', 'trust-policy-exclude'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('undefined')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', 'trustPolicyExclude'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('undefined')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', 'packages'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('undefined')
  }
})

test('pnpm config get reads workspace-specific settings from pnpm-workspace.yaml', () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    dlxCacheMaxAge: 1234,
    trustPolicyExclude: ['foo', 'bar'],
    packages: ['baz', 'qux'],
  })

  {
    const { stdout } = execPnpmSync(['config', 'get', 'dlx-cache-max-age'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('1234')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', 'dlxCacheMaxAge'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('1234')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'trust-policy-exclude'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(['foo', 'bar'])
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'trustPolicyExclude'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(['foo', 'bar'])
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'packages'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(['baz', 'qux'])
  }
})

test('pnpm config get ignores non camelCase settings from pnpm-workspace.yaml', () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    'dlx-cache-max-age': 1234,
    'trust-policy-exclude': ['foo', 'bar'],
  })

  {
    const { stdout } = execPnpmSync(['config', 'get', 'dlx-cache-max-age'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('undefined')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', 'dlxCacheMaxAge'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('undefined')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', 'trust-policy-exclude'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('undefined')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', 'trustPolicyExclude'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('undefined')
  }
})

test('pnpm config get accepts a property path', () => {
  const workspaceManifest = {
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
  } satisfies Partial<WorkspaceManifest>

  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
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

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', ''], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(expect.objectContaining({
      packageExtensions: workspaceManifest.packageExtensions,
    }))
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'packageExtensions'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(workspaceManifest.packageExtensions)
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'packageExtensions["@babel/parser"]'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(workspaceManifest.packageExtensions['@babel/parser'])
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'packageExtensions["@babel/parser"].peerDependencies'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(workspaceManifest.packageExtensions['@babel/parser'].peerDependencies)
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'packageExtensions["@babel/parser"].peerDependencies["@babel/types"]'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(workspaceManifest.packageExtensions['@babel/parser'].peerDependencies['@babel/types'])
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'packageExtensions["jest-circus"]'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(workspaceManifest.packageExtensions['jest-circus'])
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'packageExtensions["jest-circus"].dependencies'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(workspaceManifest.packageExtensions['jest-circus'].dependencies)
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '--json', 'packageExtensions["jest-circus"].dependencies.slash'], { expectSuccess: true })
    expect(JSON.parse(stdout.toString())).toStrictEqual(workspaceManifest.packageExtensions['jest-circus'].dependencies.slash)
  }
})

test('pnpm config get "" gives exactly the same result as pnpm config list', () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', {
    dlxCacheMaxAge: 1234,
    trustPolicyExclude: ['foo', 'bar'],
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

  {
    const getResult = execPnpmSync(['config', 'get', ''], { expectSuccess: true })
    const listResult = execPnpmSync(['config', 'list'], { expectSuccess: true })
    expect(getResult.stdout.toString()).toBe(listResult.stdout.toString())
  }

  {
    const getResult = execPnpmSync(['config', 'get', '--json', ''], { expectSuccess: true })
    const listResult = execPnpmSync(['config', 'list', '--json'], { expectSuccess: true })
    expect(getResult.stdout.toString()).toBe(listResult.stdout.toString())
  }
})

test('pnpm config get shows settings from global config.yaml', () => {
  prepare()

  const XDG_CONFIG_HOME = path.resolve('.config')
  const configDir = path.join(XDG_CONFIG_HOME, 'pnpm')
  fs.mkdirSync(configDir, { recursive: true })
  writeYamlFileSync(path.join(configDir, 'config.yaml'), {
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

  const configGet = (key: string) => execPnpmSync(['config', 'get', key], {
    expectSuccess: true,
    env: {
      XDG_CONFIG_HOME,
    },
  }).stdout.toString().trim()

  // lists keys that belong to global
  expect(configGet('dangerouslyAllowAllBuilds')).toBe('true')
  expect(configGet('dangerously-allow-all-builds')).toBe('true')
  expect(configGet('dlxCacheMaxAge')).toBe('1234')
  expect(configGet('dlx-cache-max-age')).toBe('1234')
  expect(configGet('globalconfig')).toBe(path.join(configDir, 'config.yaml'))

  // doesn't list CLI options
  expect(configGet('dev')).toBe('undefined')
  expect(configGet('frozenLockfile')).toBe('undefined')
  expect(configGet('frozen-lockfile')).toBe('undefined')

  // doesn't list workspace-specific keys
  expect(configGet('catalog')).toBe('undefined')
  expect(configGet('catalogs')).toBe('undefined')
  expect(configGet('packages')).toBe('undefined')
  expect(configGet('packageExtensions')).toBe('undefined')
  expect(configGet('package-extensions')).toBe('undefined')
})

// https://github.com/pnpm/pnpm/issues/16598
test('pnpm config get --global and --location=global read only the global config', () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', { nodeLinker: 'hoisted' })
  fs.writeFileSync('.npmrc', '//project.test/:_authToken=project-token\n')

  const XDG_CONFIG_HOME = path.resolve('.config')
  fs.mkdirSync(path.join(XDG_CONFIG_HOME, 'pnpm'), { recursive: true })
  writeYamlFileSync(path.join(XDG_CONFIG_HOME, 'pnpm/config.yaml'), { dlxCacheMaxAge: 1234 })
  // Reading the global config does not need the global bin directory in PATH.
  const PNPM_HOME = path.resolve('pnpm-home')
  // The global packages' manifest configures global installs, not the global config.
  fs.mkdirSync(path.join(PNPM_HOME, 'global/v11'), { recursive: true })
  writeYamlFileSync(path.join(PNPM_HOME, 'global/v11/pnpm-workspace.yaml'), { nodeLinker: 'isolated' })
  const env = { XDG_CONFIG_HOME, PNPM_HOME, [PATH_NAME]: path.resolve('bin') }
  const pnpm = (args: string[]) =>
    execPnpmSync(args, { expectSuccess: true, env }).stdout.toString().trim()

  expect(pnpm(['config', 'get', 'nodeLinker'])).toBe('hoisted')
  expect(pnpm(['config', 'get', 'nodeLinker', '--global'])).toBe('undefined')
  expect(pnpm(['config', 'get', 'nodeLinker', '--location=global'])).toBe('undefined')
  expect(pnpm(['get', 'nodeLinker', '--location=global'])).toBe('undefined')
  expect(pnpm(['config', 'get', 'nodeLinker', '--global', '--location=project'])).toBe('hoisted')
  expect(pnpm(['config', 'get', 'dlxCacheMaxAge', '--location=global'])).toBe('1234')
  expect(pnpm(['config', 'get', '//project.test/:_authToken'])).toBe('project-token')
  expect(pnpm(['config', 'get', '//project.test/:_authToken', '--global'])).toBe('undefined')
  expect(pnpm(['config', 'get', '//project.test/:_authToken', '--location=global'])).toBe('undefined')

  for (const scope of ['--global', '--location=global']) {
    const list = JSON.parse(pnpm(['config', 'list', scope]))
    expect(list).not.toHaveProperty('nodeLinker')
    expect(list).not.toHaveProperty(['//project.test/:_authToken'])
    expect(list).toHaveProperty('dlxCacheMaxAge', 1234)
  }
})

test('the path from "config get globalconfig" is the file that pnpm actually reads global settings from', () => {
  prepare()

  const XDG_CONFIG_HOME = path.resolve('.config')
  const env = { XDG_CONFIG_HOME }
  const configGet = (key: string) =>
    execPnpmSync(['config', 'get', key], { expectSuccess: true, env }).stdout.toString().trim()

  const globalConfigPath = configGet('globalconfig')
  fs.mkdirSync(path.dirname(globalConfigPath), { recursive: true })
  fs.writeFileSync(globalConfigPath, 'dlxCacheMaxAge: 4321\n')

  expect(configGet('dlx-cache-max-age')).toBe('4321')
})

test('a single-setting read prints no configuration warnings, but "config list" still does', () => {
  prepare()
  writeYamlFileSync('pnpm-workspace.yaml', { minimumReleaseAg: 100, dlxCacheMaxAge: 4321 })

  const unrecognized = 'are not recognized by this version of pnpm'

  const get = execPnpmSync(['config', 'get', 'dlx-cache-max-age'], { expectSuccess: true })
  expect(get.stdout.toString().trim()).toBe('4321')
  expect(get.stderr.toString()).not.toContain(unrecognized)

  const getAlias = execPnpmSync(['get', 'dlx-cache-max-age'], { expectSuccess: true })
  expect(getAlias.stderr.toString()).not.toContain(unrecognized)

  const list = execPnpmSync(['config', 'list'], { expectSuccess: true })
  expect(list.stderr.toString()).toContain(`${unrecognized} and were ignored: "minimumReleaseAg" (did you mean "minimumReleaseAge"?)`)
})
