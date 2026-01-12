import fs from 'fs'
import path from 'path'
import { sync as writeYamlFile } from 'write-yaml-file'
import { type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { prepare } from '@pnpm/prepare'
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

  {
    const { stdout } = execPnpmSync(['config', 'get', '@my-org:registry'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('https://my-org.registry.example.com')
  }

  {
    const { stdout } = execPnpmSync(['config', 'get', '@jsr:registry'], { expectSuccess: true })
    expect(stdout.toString().trim()).toBe('https://not-actually-jsr.example.com')
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
  writeYamlFile('pnpm-workspace.yaml', {
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
  writeYamlFile('pnpm-workspace.yaml', {
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
  writeYamlFile('pnpm-workspace.yaml', {
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
  writeYamlFile('pnpm-workspace.yaml', {
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
