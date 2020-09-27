/// <reference path="../../../typings/index.d.ts"/>
import getConfig from '@pnpm/config'
import PnpmError from '@pnpm/error'

import './findBestGlobalPrefixOnWindows'
import fs = require('mz/fs')
import path = require('path')
import tempy = require('tempy')

// To override any local settings,
// we force the default values of config
delete process.env.npm_config_depth
process.env['npm_config_hoist'] = 'true'
delete process.env.npm_config_registry
delete process.env.npm_config_virtual_store_dir
delete process.env.npm_config_shared_workspace_lockfile

test('getConfig()', async () => {
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config).toBeDefined()
  expect(config.fetchRetries).toEqual(2)
  expect(config.fetchRetryFactor).toEqual(10)
  expect(config.fetchRetryMintimeout).toEqual(10000)
  expect(config.fetchRetryMaxtimeout).toEqual(60000)
})

test('throw error if --link-workspace-packages is used with --global', async () => {
  try {
    await getConfig({
      cliOptions: {
        global: true,
        'link-workspace-packages': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('Configuration conflict. "link-workspace-packages" may not be used with "global"')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL')
  }
})

test('"save" should always be true during global installation', async () => {
  const { config } = await getConfig({
    cliOptions: {
      global: true,
      save: false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.save).toBeTruthy()
})

test('throw error if --shared-workspace-lockfile is used with --global', async () => {
  try {
    await getConfig({
      cliOptions: {
        global: true,
        'shared-workspace-lockfile': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('Configuration conflict. "shared-workspace-lockfile" may not be used with "global"')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL')
  }
})

test('throw error if --lockfile-dir is used with --global', async () => {
  try {
    await getConfig({
      cliOptions: {
        global: true,
        'lockfile-dir': '/home/src',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('Configuration conflict. "lockfile-dir" may not be used with "global"')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL')
  }
})

test('throw error if --hoist-pattern is used with --global', async () => {
  try {
    await getConfig({
      cliOptions: {
        global: true,
        'hoist-pattern': 'eslint',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('Configuration conflict. "hoist-pattern" may not be used with "global"')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL')
  }
})

test('throw error if --virtual-store-dir is used with --global', async () => {
  try {
    await getConfig({
      cliOptions: {
        global: true,
        'virtual-store-dir': 'pkgs',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('Configuration conflict. "virtual-store-dir" may not be used with "global"')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL')
  }
})

test('when using --global, link-workspace-packages, shared-workspace-shrinwrap and lockfile-directory are false even if it is set to true in a .npmrc file', async () => {
  const tmp = tempy.directory()

  process.chdir(tmp)
  const npmrc = [
    'link-workspace-packages=true',
    'shared-workspace-lockfile=true',
    'lockfile-directory=/home/src',
  ].join('\n')
  await fs.writeFile('.npmrc', npmrc, 'utf8')
  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  {
    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
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
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    expect(config.linkWorkspacePackages).toBeFalsy()
    expect(config.sharedWorkspaceLockfile).toBeFalsy()
    // FIXME: it supposed to return null but is undefined
    expect(config.lockfileDir).toBeUndefined()
  }
})

test('registries of scoped packages are read', async () => {
  const { config } = await getConfig({
    cliOptions: {
      dir: 'workspace',
      userconfig: path.join(__dirname, 'scoped-registries.ini'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.registries).toStrictEqual({
    default: 'https://default.com/',
    '@foo': 'https://foo.com/',
    '@bar': 'https://bar.com/',
  })
})

test('registries in current directory\'s .npmrc have bigger priority then global config settings', async () => {
  const tmp = tempy.directory()

  process.chdir(tmp)
  await fs.writeFile('.npmrc', 'registry=https://pnpm.js.org/', 'utf8')

  const { config } = await getConfig({
    cliOptions: {
      userconfig: path.join(__dirname, 'scoped-registries.ini'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.registries).toStrictEqual({
    default: 'https://pnpm.js.org/',
    '@foo': 'https://foo.com/',
    '@bar': 'https://bar.com/',
  })
})

test('filter is read from .npmrc as an array', async () => {
  const tmp = tempy.directory()

  process.chdir(tmp)
  await fs.writeFile('.npmrc', 'filter=foo bar...', 'utf8')
  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  const { config } = await getConfig({
    cliOptions: {
      global: false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.filter).toStrictEqual(['foo', 'bar...'])
})

test('throw error if --save-prod is used with --save-peer', async () => {
  try {
    await getConfig({
      cliOptions: {
        'save-peer': true,
        'save-prod': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('A package cannot be a peer dependency and a prod dependency at the same time')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_PEER_CANNOT_BE_PROD_DEP')
  }
})

test('throw error if --save-optional is used with --save-peer', async () => {
  try {
    await getConfig({
      cliOptions: {
        'save-optional': true,
        'save-peer': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('A package cannot be a peer dependency and an optional dependency at the same time')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_PEER_CANNOT_BE_OPTIONAL_DEP')
  }
})

test('extraBinPaths', async () => {
  const tmp = tempy.directory()

  process.chdir(tmp)

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

test('hoist-pattern is undefined if --no-hoist used', async () => {
  const { config } = await getConfig({
    cliOptions: {
      hoist: false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.hoistPattern).toBeUndefined()
})

test('throw error if --no-hoist is used with --shamefully-hoist', async () => {
  try {
    await getConfig({
      cliOptions: {
        hoist: false,
        'shamefully-hoist': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('--shamefully-hoist cannot be used with --no-hoist')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_HOIST')
  }
})

test('throw error if --no-hoist is used with --shamefully-flatten', async () => {
  try {
    await getConfig({
      cliOptions: {
        hoist: false,
        'shamefully-flatten': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('--shamefully-flatten cannot be used with --no-hoist')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_HOIST')
  }
})

test('throw error if --no-hoist is used with --hoist-pattern', async () => {
  try {
    await getConfig({
      cliOptions: {
        hoist: false,
        'hoist-pattern': 'eslint-*',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    expect(err.message).toEqual('--hoist-pattern cannot be used with --no-hoist')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_HOIST')
  }
})

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

    expect(config.publicHoistPattern).toBeUndefined()
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

    expect(config.publicHoistPattern).toBeUndefined()
  }
})

test.skip('rawLocalConfig in a workspace', async () => {
  const tmp = tempy.directory()

  process.chdir(tmp)
  const workspaceDir = process.cwd()
  await fs.writeFile('.npmrc', 'hoist-pattern=*', 'utf8')
  await fs.mkdir('package')
  process.chdir('package')
  await fs.writeFile('.npmrc', 'hoist-pattern=eslint-*', 'utf8')

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
  await fs.mkdir('package2')
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
  const tmp = tempy.directory()

  process.chdir(tmp)
  await fs.writeFile('.npmrc', 'modules-dir=modules', 'utf8')

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

    expect(config.color).toEqual('always')
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

    expect(config.color).toEqual('never')
  }
})

test('read only supported settings from config', async () => {
  const tmp = tempy.directory()

  process.chdir(tmp)
  await fs.writeFile('.npmrc', 'store-dir=__store__\nfoo=bar', 'utf8')

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.storeDir).toEqual('__store__')
  expect(config['foo']).toBeUndefined()
  expect(config.rawConfig['foo']).toEqual('bar')
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

  expect(config['fooBar']).toEqual('qar')
})

test('local prefix search stops on pnpm-workspace.yaml', async () => {
  const workspaceDir = path.join(__dirname, 'has-workspace-yaml')
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
