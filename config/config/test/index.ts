/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import os from 'os'
import path from 'path'
import PATH from 'path-name'
import { sync as writeYamlFile } from 'write-yaml-file'
import loadNpmConf from '@pnpm/npm-conf'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { jest } from '@jest/globals'

import symlinkDir from 'symlink-dir'

jest.unstable_mockModule('@pnpm/git-utils', () => ({ getCurrentBranch: jest.fn() }))

const { getConfig } = await import('@pnpm/config')
const { getCurrentBranch } = await import('@pnpm/git-utils')

// To override any local settings,
// we force the default values of config
process.env['npm_config_hoist'] = 'true'
process.env['pnpm_config_hoist'] = 'true'
for (const suffix of [
  'depth',
  'registry',
  'virtual_store_dir',
  'shared_workspace_lockfile',
  'node_version',
  'fetch_retries',
]) {
  delete process.env[`npm_config_${suffix}`]
  delete process.env[`pnpm_config_${suffix}`]
}

const env = {
  PNPM_HOME: import.meta.dirname,
  [PATH]: import.meta.dirname,
}
const f = fixtures(import.meta.dirname)

test('getConfig()', async () => {
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config).toBeDefined()
  expect(config.fetchRetries).toBe(2)
  expect(config.fetchRetryFactor).toBe(10)
  expect(config.fetchRetryMintimeout).toBe(10000)
  expect(config.fetchRetryMaxtimeout).toBe(60000)
  // nodeVersion should not have a default value.
  // When not specified, the package-is-installable package detects nodeVersion automatically.
  expect(config.nodeVersion).toBeUndefined()
})

test('throw error if --link-workspace-packages is used with --global', async () => {
  await expect(getConfig({
    cliOptions: {
      global: true,
      'link-workspace-packages': true,
    },
    env,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL',
    message: 'Configuration conflict. "link-workspace-packages" may not be used with "global"',
  })
})

test('correct settings on global install', async () => {
  const { config } = await getConfig({
    cliOptions: {
      global: true,
      save: false,
    },
    env,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.save).toBe(true)
})

test('throw error if --shared-workspace-lockfile is used with --global', async () => {
  await expect(getConfig({
    cliOptions: {
      global: true,
      'shared-workspace-lockfile': true,
    },
    env,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL',
    message: 'Configuration conflict. "shared-workspace-lockfile" may not be used with "global"',
  })
})

test('throw error if --lockfile-dir is used with --global', async () => {
  await expect(getConfig({
    cliOptions: {
      global: true,
      'lockfile-dir': '/home/src',
    },
    env,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL',
    message: 'Configuration conflict. "lockfile-dir" may not be used with "global"',
  })
})

test('throw error if --hoist-pattern is used with --global', async () => {
  await expect(getConfig({
    cliOptions: {
      global: true,
      'hoist-pattern': 'eslint',
    },
    env,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL',
    message: 'Configuration conflict. "hoist-pattern" may not be used with "global"',
  })
})

test('throw error if --virtual-store-dir is used with --global', async () => {
  await expect(getConfig({
    cliOptions: {
      global: true,
      'virtual-store-dir': 'pkgs',
    },
    env,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL',
    message: 'Configuration conflict. "virtual-store-dir" may not be used with "global"',
  })
})

test('.npmrc does not load pnpm settings', async () => {
  prepareEmpty()

  const npmrc = [
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
  ].join('\n')
  fs.writeFileSync('.npmrc', npmrc)

  const { config } = await getConfig({
    cliOptions: {
      global: false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  // rc options appear as usual
  expect(config.rawConfig).toMatchObject({
    '//my-org.registry.example.com:username': 'some-employee',
    '//my-org.registry.example.com:_authToken': 'some-employee-token',
    '@my-org:registry': 'https://my-org.registry.example.com',
    '@jsr:registry': 'https://not-actually-jsr.example.com',
    username: 'example-user-name',
    _authToken: 'example-auth-token',
  })

  // workspace-specific settings are omitted
  expect(config.rawConfig['dlx-cache-max-age']).toBeUndefined()
  expect(config.rawConfig['dlxCacheMaxAge']).toBeUndefined()
  expect(config.dlxCacheMaxAge).toBe(24 * 60) // TODO: refactor to make defaultOptions importable
  expect(config.rawConfig['trust-policy-exclude']).toBeUndefined()
  expect(config.rawConfig['trustPolicyExclude']).toBeUndefined()
  expect(config.trustPolicyExclude).toBeUndefined()
  expect(config.rawConfig.packages).toBeUndefined()
})

test('rc options appear as kebab-case in rawConfig even if it was defined as camelCase by pnpm-workspace.yaml', async () => {
  prepareEmpty()

  writeYamlFile('pnpm-workspace.yaml', {
    ignoreScripts: true,
    linkWorkspacePackages: true,
    nodeLinker: 'hoisted',
    sharedWorkspaceLockfile: true,
  })

  const { config } = await getConfig({
    cliOptions: {
      global: false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })

  expect(config).toMatchObject({
    ignoreScripts: true,
    linkWorkspacePackages: true,
    nodeLinker: 'hoisted',
    sharedWorkspaceLockfile: true,
    rawConfig: {
      'ignore-scripts': true,
      'link-workspace-packages': true,
      'node-linker': 'hoisted',
      'shared-workspace-lockfile': true,
    },
  })

  expect(config.rawConfig.ignoreScripts).toBeUndefined()
  expect(config.rawConfig.linkWorkspacePackages).toBeUndefined()
  expect(config.rawConfig.nodeLinker).toBeUndefined()
  expect(config.rawConfig.sharedWorkspaceLockfile).toBeUndefined()
})

test('workspace-specific settings preserve case in rawConfig', async () => {
  prepareEmpty()

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['foo', 'bar'],
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

  const { config } = await getConfig({
    cliOptions: {
      global: false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })

  expect(config.rawConfig.packages).toStrictEqual(['foo', 'bar'])
  expect(config.rawConfig.packageExtensions).toStrictEqual({
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
  })
  expect(config.rawConfig['package-extensions']).toBeUndefined()
  expect(config.packageExtensions).toStrictEqual({
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
  })
})

test('when using --global, linkWorkspacePackages, sharedWorkspaceLockfile and lockfileDir are false even if they are set to true in pnpm-workspace.yaml', async () => {
  prepareEmpty()

  writeYamlFile('pnpm-workspace.yaml', {
    linkWorkspacePackages: true,
    sharedWorkspaceLockfile: true,
    lockfileDir: true,
  })

  {
    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })
    expect(config.linkWorkspacePackages).toBeTruthy()
    expect(config.sharedWorkspaceLockfile).toBeTruthy()
    expect(config.lockfileDir).toBeTruthy()
  }

  {
    const { config } = await getConfig({
      cliOptions: {
        global: true,
      },
      env,
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })
    expect(config.linkWorkspacePackages).toBeFalsy()
    expect(config.sharedWorkspaceLockfile).toBeFalsy()
    // FIXME: it supposed to return null but is undefined
    expect(config.lockfileDir).toBeUndefined()
  }
})

test('registries of scoped packages are read and normalized', async () => {
  const { config } = await getConfig({
    cliOptions: {
      userconfig: path.join(import.meta.dirname, 'scoped-registries.ini'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.registries).toStrictEqual({
    default: 'https://default.com/',
    '@jsr': 'https://npm.jsr.io/',
    '@foo': 'https://foo.com/',
    '@bar': 'https://bar.com/',
    '@qar': 'https://qar.com/qar',
  })
})

test('registries in current directory\'s .npmrc have bigger priority then global config settings', async () => {
  prepare()

  fs.writeFileSync('.npmrc', 'registry=https://pnpm.io/', 'utf8')

  const { config } = await getConfig({
    cliOptions: {
      userconfig: path.join(import.meta.dirname, 'scoped-registries.ini'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.registries).toStrictEqual({
    default: 'https://pnpm.io/',
    '@jsr': 'https://npm.jsr.io/',
    '@foo': 'https://foo.com/',
    '@bar': 'https://bar.com/',
    '@qar': 'https://qar.com/qar',
  })
})

test('throw error if --save-prod is used with --save-peer', async () => {
  await expect(getConfig({
    cliOptions: {
      'save-peer': true,
      'save-prod': true,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_PEER_CANNOT_BE_PROD_DEP',
    message: 'A package cannot be a peer dependency and a prod dependency at the same time',
  })
})

test('throw error if --save-optional is used with --save-peer', async () => {
  await expect(getConfig({
    cliOptions: {
      'save-optional': true,
      'save-peer': true,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_PEER_CANNOT_BE_OPTIONAL_DEP',
    message: 'A package cannot be a peer dependency and an optional dependency at the same time',
  })
})

test('extraBinPaths', async () => {
  prepareEmpty()

  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    // extraBinPaths is empty outside of a workspace
    expect(config.extraBinPaths).toHaveLength(0)
  }

  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })
    // extraBinPaths has the node_modules/.bin folder from the root of the workspace
    expect(config.extraBinPaths).toStrictEqual([path.resolve('node_modules/.bin')])
  }

  {
    const { config } = await getConfig({
      cliOptions: {
        'ignore-scripts': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })
    // extraBinPaths has the node_modules/.bin folder from the root of the workspace if scripts are ignored
    expect(config.extraBinPaths).toStrictEqual([path.resolve('node_modules/.bin')])
  }

  {
    const { config } = await getConfig({
      cliOptions: {
        'ignore-scripts': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    // extraBinPaths is empty inside a workspace if scripts are ignored
    expect(config.extraBinPaths).toEqual([])
  }
})

test('convert shamefully-flatten to hoist-pattern=* and warn', async () => {
  const { config, warnings } = await getConfig({
    cliOptions: {
      'shamefully-flatten': true,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.hoistPattern).toStrictEqual(['*'])
  expect(config.shamefullyHoist).toBeTruthy()
  expect(warnings).toStrictEqual([
    'The "shamefully-flatten" setting has been renamed to "shamefully-hoist". ' +
    'Also, in most cases you won\'t need "shamefully-hoist". ' +
    'Since v4, a semistrict node_modules structure is on by default (via hoist-pattern=[*]).',
  ])
})

// hoist → hoistPattern processing is done in @pnpm/cli-utils
test('hoist-pattern is unchanged if --no-hoist used', async () => {
  const { config } = await getConfig({
    cliOptions: {
      hoist: false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.hoist).toBe(false)
  expect(config.hoistPattern).toStrictEqual(['*'])
})

test('throw error if --no-hoist is used with --shamefully-hoist', async () => {
  await expect(getConfig({
    cliOptions: {
      hoist: false,
      'shamefully-hoist': true,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_HOIST',
    message: '--shamefully-hoist cannot be used with --no-hoist',
  })
})

test('throw error if --no-hoist is used with --shamefully-flatten', async () => {
  await expect(getConfig({
    cliOptions: {
      hoist: false,
      'shamefully-flatten': true,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_HOIST',
    message: '--shamefully-flatten cannot be used with --no-hoist',
  })
})

test('throw error if --no-hoist is used with --hoist-pattern', async () => {
  await expect(getConfig({
    cliOptions: {
      hoist: false,
      'hoist-pattern': 'eslint-*',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_CONFIG_CONFLICT_HOIST',
    message: '--hoist-pattern cannot be used with --no-hoist',
  })
})

// public-hoist-pattern normalization is done in @pnpm/cli-utils
test('normalizing the value of public-hoist-pattern', async () => {
  {
    const { config } = await getConfig({
      cliOptions: {
        'public-hoist-pattern': '',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.publicHoistPattern).toBe('')
  }
  {
    const { config } = await getConfig({
      cliOptions: {
        'public-hoist-pattern': [''],
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.publicHoistPattern).toStrictEqual([''])
  }
})

test.skip('rawLocalConfig in a workspace', async () => {
  prepareEmpty()

  const workspaceDir = process.cwd()
  fs.writeFileSync('.npmrc', 'hoist-pattern=*', 'utf8')
  fs.mkdirSync('package')
  process.chdir('package')
  fs.writeFileSync('.npmrc', 'hoist-pattern=eslint-*', 'utf8')

  {
    const { config } = await getConfig({
      cliOptions: {
        'save-exact': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir,
    })

    expect(config.rawLocalConfig).toStrictEqual({
      'hoist-pattern': 'eslint-*',
      'save-exact': true,
    })
  }

  // package w/o its own .npmrc
  fs.mkdirSync('package2')
  process.chdir('package2')
  {
    const { config } = await getConfig({
      cliOptions: {
        'save-exact': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir,
    })

    expect(config.rawLocalConfig).toStrictEqual({
      'hoist-pattern': '*',
      'save-exact': true,
    })
  }
})

test.skip('rawLocalConfig', async () => {
  prepareEmpty()

  fs.writeFileSync('.npmrc', 'modules-dir=modules', 'utf8')

  const { config } = await getConfig({
    cliOptions: {
      'save-exact': true,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.rawLocalConfig).toStrictEqual({
    'modules-dir': 'modules',
    'save-exact': true,
  })
})

test('normalize the value of the color flag', async () => {
  {
    const { config } = await getConfig({
      cliOptions: {
        color: true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.color).toBe('always')
  }
  {
    const { config } = await getConfig({
      cliOptions: {
        color: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.color).toBe('never')
  }
})

// NOTE: This test currently fails as pnpm currently lack a way to verify pnpm-workspace.yaml
test.skip('read only supported settings from config', async () => {
  prepare()

  writeYamlFile('pnpm-workspace.yaml', {
    storeDir: '__store__',
    foo: 'bar',
  })

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })

  expect(config.storeDir).toBe('__store__')
  // @ts-expect-error
  expect(config['foo']).toBeUndefined() // NOTE: This line current fails as there are yet a way to verify fields in pnpm-workspace.yaml
  expect(config.rawConfig['foo']).toBe('bar')
})

test('all CLI options are added to the config', async () => {
  const { config } = await getConfig({
    cliOptions: {
      'foo-bar': 'qar',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  // @ts-expect-error
  expect(config['fooBar']).toBe('qar')
})

test('local prefix search stops on pnpm-workspace.yaml', async () => {
  const workspaceDir = path.join(import.meta.dirname, 'has-workspace-yaml')
  process.chdir(workspaceDir)
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.dir).toEqual(workspaceDir)
})

test('reads workspacePackagePatterns', async () => {
  const workspaceDir = path.join(import.meta.dirname, 'fixtures/pkg-with-valid-workspace-yaml')
  process.chdir(workspaceDir)
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir,
  })

  expect(config.workspacePackagePatterns).toEqual(['packages/*'])
})

test('setting workspace-concurrency to negative number', async () => {
  const workspaceDir = path.join(import.meta.dirname, 'fixtures/pkg-with-valid-workspace-yaml')
  process.chdir(workspaceDir)
  const { config } = await getConfig({
    cliOptions: {
      'workspace-concurrency': -1,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir,
  })
  expect(config.workspaceConcurrency >= 1).toBeTruthy()
})

test('respects testPattern', async () => {
  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.testPattern).toBeUndefined()
  }
  {
    const workspaceDir = path.join(import.meta.dirname, 'using-test-pattern')
    process.chdir(workspaceDir)
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir,
    })

    expect(config.testPattern).toEqual(['*.spec.js', '*.spec.ts'])
  }
  {
    const workspaceDir = path.join(import.meta.dirname, 'ignore-test-pattern')
    process.chdir(workspaceDir)
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir,
    })

    expect(config.testPattern).toBeUndefined()
  }
})

test('respects changedFilesIgnorePattern', async () => {
  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.changedFilesIgnorePattern).toBeUndefined()
  }
  {
    prepareEmpty()

    writeYamlFile('pnpm-workspace.yaml', {
      changedFilesIgnorePattern: ['.github/**', '**/README.md'],
    })

    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.changedFilesIgnorePattern).toEqual(['.github/**', '**/README.md'])
  }
})

test('dir is resolved to real path', async () => {
  prepareEmpty()
  const realDir = path.resolve('real-path')
  fs.mkdirSync(realDir)
  const symlink = path.resolve('symlink')
  await symlinkDir(realDir, symlink)
  const { config } = await getConfig({
    cliOptions: { dir: symlink },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.dir).toBe(realDir)
})

test('warn user unknown settings in npmrc', async () => {
  prepare()

  const npmrc = [
    'typo-setting=true',
    ' ',
    'mistake-setting=false',
    '//foo.bar:_authToken=aaa',
    '@qar:registry=https://registry.example.org/',
  ].join('\n')
  fs.writeFileSync('.npmrc', npmrc, 'utf8')

  const { warnings } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    checkUnknownSetting: true,
  })

  expect(warnings).toStrictEqual([
    'Your .npmrc file contains unknown setting: typo-setting, mistake-setting',
  ])

  const { warnings: noWarnings } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(noWarnings).toStrictEqual([])
})

test('getConfig() converts noproxy to noProxy', async () => {
  const { config } = await getConfig({
    cliOptions: {
      noproxy: 'www.foo.com',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.noProxy).toBe('www.foo.com')
})

test('getConfig() returns the userconfig', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry = https://registry.example.test', 'utf-8')
  loadNpmConf.defaults.userconfig = path.resolve('user-home', '.npmrc')
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.userConfig).toEqual({ registry: 'https://registry.example.test' })
})

test('getConfig() returns the userconfig even when overridden locally', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry = https://registry.example.test', 'utf-8')
  loadNpmConf.defaults.userconfig = path.resolve('user-home', '.npmrc')
  fs.writeFileSync('.npmrc', 'registry = https://project-local.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.registry).toBe('https://project-local.example.test')
  expect(config.userConfig).toEqual({ registry: 'https://registry.example.test' })
})

test('getConfig() sets sideEffectsCacheRead and sideEffectsCacheWrite when side-effects-cache is set', async () => {
  const { config } = await getConfig({
    cliOptions: {
      'side-effects-cache': true,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config).toBeDefined()
  expect(config.sideEffectsCacheRead).toBeTruthy()
  expect(config.sideEffectsCacheWrite).toBeTruthy()
})

test('getConfig() should read cafile', async () => {
  const { config } = await getConfig({
    cliOptions: {
      cafile: path.join(import.meta.dirname, 'cafile.txt'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config).toBeDefined()
  expect(config.ca).toStrictEqual([`xxx
-----END CERTIFICATE-----`])
})

test('getConfig() should read inline SSL certificates from .npmrc', async () => {
  prepareEmpty()

  // These are written to .npmrc with literal \n strings
  const inlineCa = '-----BEGIN CERTIFICATE-----\\nMIIFNzCCAx+gAwIBAgIQNB613yRzpKtDztlXiHmOGDANBgkqhkiG9w0BAQsFADAR\\n-----END CERTIFICATE-----'
  const inlineCert = '-----BEGIN CERTIFICATE-----\\nMIIClientCert\\n-----END CERTIFICATE-----'
  const inlineKey = '-----BEGIN PRIVATE KEY-----\\nMIIClientKey\\n-----END PRIVATE KEY-----'

  const npmrc = [
    '//registry.example.com/:ca=' + inlineCa,
    '//registry.example.com/:cert=' + inlineCert,
    '//registry.example.com/:key=' + inlineKey,
  ].join('\n')
  fs.writeFileSync('.npmrc', npmrc, 'utf8')

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  // After processing, \n should be converted to actual newlines
  expect(config.sslConfigs).toBeDefined()
  expect(config.sslConfigs['//registry.example.com/']).toStrictEqual({
    ca: inlineCa.replace(/\\n/g, '\n'),
    cert: inlineCert.replace(/\\n/g, '\n'),
    key: inlineKey.replace(/\\n/g, '\n'),
  })
})

test('respect mergeGitBranchLockfilesBranchPattern', async () => {
  {
    prepareEmpty()
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.mergeGitBranchLockfilesBranchPattern).toBeUndefined()
    expect(config.mergeGitBranchLockfiles).toBeUndefined()
  }
  {
    prepareEmpty()

    writeYamlFile('pnpm-workspace.yaml', {
      mergeGitBranchLockfilesBranchPattern: ['main', 'release/**'],
    })

    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.mergeGitBranchLockfilesBranchPattern).toEqual(['main', 'release/**'])
  }
})

test('getConfig() sets mergeGitBranchLockfiles when branch matches mergeGitBranchLockfilesBranchPattern', async () => {
  prepareEmpty()
  {
    writeYamlFile('pnpm-workspace.yaml', {
      mergeGitBranchLockfilesBranchPattern: ['main', 'release/**'],
    })

    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('develop'))
    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.mergeGitBranchLockfilesBranchPattern).toEqual(['main', 'release/**'])
    expect(config.mergeGitBranchLockfiles).toBe(false)
  }
  {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('main'))
    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })
    expect(config.mergeGitBranchLockfiles).toBe(true)
  }
  {
    jest.mocked(getCurrentBranch).mockReturnValue(Promise.resolve('release/1.0.0'))
    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })
    expect(config.mergeGitBranchLockfiles).toBe(true)
  }
})

test('preferSymlinkedExecutables should be true when nodeLinker is hoisted', async () => {
  prepareEmpty()

  const { config } = await getConfig({
    cliOptions: {
      'node-linker': 'hoisted',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.preferSymlinkedExecutables).toBeTruthy()
})

test('return a warning when the .npmrc has an env variable that does not exist', async () => {
  fs.writeFileSync('.npmrc', 'registry=${ENV_VAR_123}', 'utf8')
  const { warnings } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  const expected = [
    expect.stringContaining('Failed to replace env in config: ${ENV_VAR_123}') // eslint-disable-line
  ]

  expect(warnings).toEqual(expect.arrayContaining(expected))
})

test('getConfig() returns failedToLoadBuiltInConfig', async () => {
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.failedToLoadBuiltInConfig).toBeDefined()
})

test('return a warning if a package.json has workspaces field but there is no pnpm-workspaces.yaml file', async () => {
  const prefix = f.find('pkg-using-workspaces')
  const { warnings } = await getConfig({
    cliOptions: { dir: prefix },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(warnings).toStrictEqual([
    'The "workspaces" field in package.json is not supported by pnpm. Create a "pnpm-workspace.yaml" file instead.',
  ])
})

test('do not return a warning if a package.json has workspaces field and there is a pnpm-workspace.yaml file', async () => {
  const prefix = f.find('pkg-using-workspaces')
  const { warnings } = await getConfig({
    cliOptions: { dir: prefix },
    workspaceDir: prefix,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(warnings).toStrictEqual([])
})

test('read PNPM_HOME defined in environment variables', async () => {
  const oldEnv = process.env
  const homeDir = './specified-dir'
  process.env = {
    ...oldEnv,
    PNPM_HOME: homeDir,
  }

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.pnpmHomeDir).toBe(homeDir)

  process.env = oldEnv
})

test('xxx', async () => {
  const oldEnv = process.env
  process.env = {
    ...oldEnv,
    FOO: 'registry',
  }

  const { config } = await getConfig({
    cliOptions: {
      dir: f.find('has-env-in-key'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.registry).toBe('https://registry.example.com/')

  process.env = oldEnv
})

test('settings from pnpm-workspace.yaml are read', async () => {
  const workspaceDir = f.find('settings-in-workspace-yaml')
  process.chdir(workspaceDir)
  const { config } = await getConfig({
    cliOptions: {},
    workspaceDir,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.trustPolicyExclude).toStrictEqual(['foo', 'bar'])
  expect(config.rawConfig['trust-policy-exclude']).toStrictEqual(['foo', 'bar'])
})

test('settings sharedWorkspaceLockfile in pnpm-workspace.yaml should take effect', async () => {
  const workspaceDir = f.find('settings-in-workspace-yaml')
  process.chdir(workspaceDir)
  const { config } = await getConfig({
    cliOptions: {},
    workspaceDir,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.sharedWorkspaceLockfile).toBe(false)
  expect(config.lockfileDir).toBeUndefined()
})

// shamefullyHoist → publicHoistPattern conversion is done in @pnpm/cli-utils
test('settings shamefullyHoist in pnpm-workspace.yaml should take effect', async () => {
  const workspaceDir = f.find('settings-in-workspace-yaml')
  process.chdir(workspaceDir)
  const { config } = await getConfig({
    cliOptions: {},
    workspaceDir,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.shamefullyHoist).toBe(true)
  expect(config.rawConfig['shamefully-hoist']).toBe(true)
})

test('settings gitBranchLockfile in pnpm-workspace.yaml should take effect', async () => {
  const workspaceDir = f.find('settings-in-workspace-yaml')
  process.chdir(workspaceDir)
  const { config } = await getConfig({
    cliOptions: {},
    workspaceDir,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.gitBranchLockfile).toBe(true)
  expect(config.useGitBranchLockfile).toBe(true)
  expect(config.rawConfig['git-branch-lockfile']).toBe(true)
})

test('loads setting from environment variable pnpm_config_*', async () => {
  prepareEmpty()
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      pnpm_config_fetch_retries: '100',
      pnpm_config_hoist_pattern: '["react", "react-dom"]',
      pnpm_config_use_node_version: '22.0.0',
      pnpm_config_trust_policy_exclude: '["foo", "bar"]',
      pnpm_config_registry: 'https://registry.example.com',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })
  expect(config.fetchRetries).toBe(100)
  expect(config.hoistPattern).toStrictEqual(['react', 'react-dom'])
  expect(config.trustPolicyExclude).toStrictEqual(['foo', 'bar'])
  expect(config.registry).toBe('https://registry.example.com/')
  expect(config.registries.default).toBe('https://registry.example.com/')
})

test('environment variable pnpm_config_* should override pnpm-workspace.yaml', async () => {
  prepareEmpty()

  writeYamlFile('pnpm-workspace.yaml', {
    fetchRetries: 5,
  })

  async function getConfigValue (env: NodeJS.ProcessEnv): Promise<number | undefined> {
    const { config } = await getConfig({
      cliOptions: {},
      env,
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })
    return config.fetchRetries
  }

  expect(await getConfigValue({})).toBe(5)
  expect(await getConfigValue({
    pnpm_config_fetch_retries: '10',
  })).toBe(10)
})

test('CLI should override environment variable pnpm_config_*', async () => {
  prepareEmpty()

  async function getConfigValue (cliOptions: Record<string, unknown>): Promise<number | undefined> {
    const { config } = await getConfig({
      cliOptions,
      env: {
        pnpm_config_fetch_retries: '5',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })
    return config.fetchRetries
  }

  expect(await getConfigValue({})).toBe(5)
  expect(await getConfigValue({
    fetchRetries: 10,
  })).toBe(10)
  expect(await getConfigValue({
    'fetch-retries': 10,
  })).toBe(10)
})

test('warn when directory contains PATH delimiter character', async () => {
  const tempDir = path.join(os.tmpdir(), `pnpm-test${path.delimiter}project-${Date.now()}`)
  fs.mkdirSync(tempDir, { recursive: true })

  try {
    const { warnings } = await getConfig({
      cliOptions: { dir: tempDir },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(warnings).toContainEqual(
      expect.stringContaining('path delimiter character')
    )
  } finally {
    fs.rmSync(tempDir, { recursive: true })
  }
})

test('no warning when directory does not contain PATH delimiter character', async () => {
  const tempDir = path.join(os.tmpdir(), `pnpm-test-normal-${Date.now()}`)
  fs.mkdirSync(tempDir, { recursive: true })

  try {
    const { warnings } = await getConfig({
      cliOptions: { dir: tempDir },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(warnings).not.toContainEqual(
      expect.stringContaining('path delimiter character')
    )
  } finally {
    fs.rmSync(tempDir, { recursive: true })
  }
})

test.each([
  [undefined, undefined],
  [false, undefined],
  [true, true],
])('sets autoConfirmAllPrompts when CLI is passed --yes=%s', async (cliValue?: boolean, expectedValue?: boolean) => {
  const { config } = await getConfig({
    cliOptions: {
      'yes': cliValue,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.autoConfirmAllPrompts).toBe(expectedValue)
})

describe('global config.yaml', () => {
  let XDG_CONFIG_HOME: string | undefined

  beforeEach(() => {
    XDG_CONFIG_HOME = process.env.XDG_CONFIG_HOME
  })

  afterEach(() => {
    process.env.XDG_CONFIG_HOME = XDG_CONFIG_HOME
  })

  test('reads config from global config.yaml', async () => {
    prepareEmpty()

    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFile('.config/pnpm/config.yaml', {
      dangerouslyAllowAllBuilds: true,
    })

    // TODO: `getConfigDir`, `getHomeDir`, etc. (from dirs.ts) should allow customizing env or process.
    // TODO: after that, remove this `describe` wrapper.
    process.env.XDG_CONFIG_HOME = path.resolve('.config')

    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.dangerouslyAllowAllBuilds).toBe(true)

    // NOTE: the field may appear kebab-case here, but only internally,
    //       `pnpm config list` would convert them to camelCase.
    // TODO: switch to camelCase entirely later.
    expect(config.rawConfig).toHaveProperty(['dangerously-allow-all-builds'])
  })
})

test('lockfile: false in pnpm-workspace.yaml sets useLockfile to false', async () => {
  prepareEmpty()

  writeYamlFile('pnpm-workspace.yaml', {
    lockfile: false,
  })

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })

  expect(config.useLockfile).toBe(false)
})

test('pnpm_config_lockfile env var overrides lockfile from pnpm-workspace.yaml in useLockfile', async () => {
  prepareEmpty()

  writeYamlFile('pnpm-workspace.yaml', {
    lockfile: true,
  })

  const { config } = await getConfig({
    cliOptions: {},
    env: {
      pnpm_config_lockfile: 'false',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })

  expect(config.useLockfile).toBe(false)
})

test('pnpm_config_git_branch_lockfile env var overrides git-branch-lockfile from pnpm-workspace.yaml in useGitBranchLockfile', async () => {
  prepareEmpty()

  writeYamlFile('pnpm-workspace.yaml', {
    gitBranchLockfile: false,
  })

  const { config } = await getConfig({
    cliOptions: {},
    env: {
      pnpm_config_git_branch_lockfile: 'true',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })

  expect(config.useGitBranchLockfile).toBe(true)
})
