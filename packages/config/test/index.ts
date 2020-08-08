///<reference path="../../../typings/index.d.ts"/>
import getConfig from '@pnpm/config'
import PnpmError from '@pnpm/error'
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')

import './findBestGlobalPrefixOnWindows'

// To override any local settings,
// we force the default values of config
delete process.env['npm_config_depth']
process.env['npm_config_hoist'] = 'true'
delete process.env['npm_config_registry']
delete process.env['npm_config_virtual_store_dir']
delete process.env['npm_config_shared_workspace_lockfile']

test('getConfig()', async (t) => {
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  t.ok(config)
  t.equal(config.fetchRetries, 2)
  t.equal(config.fetchRetryFactor, 10)
  t.equal(config.fetchRetryMintimeout, 10000)
  t.equal(config.fetchRetryMaxtimeout, 60000)
  t.end()
})

test('throw error if --link-workspace-packages is used with --global', async (t) => {
  try {
    await getConfig({
      cliOptions: {
        'global': true,
        'link-workspace-packages': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, 'Configuration conflict. "link-workspace-packages" may not be used with "global"')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL')
    t.end()
  }
})

test('throw error if --shared-workspace-lockfile is used with --global', async (t) => {
  try {
    await getConfig({
      cliOptions: {
        'global': true,
        'shared-workspace-lockfile': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, 'Configuration conflict. "shared-workspace-lockfile" may not be used with "global"')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL')
    t.end()
  }
})

test('throw error if --lockfile-dir is used with --global', async (t) => {
  try {
    await getConfig({
      cliOptions: {
        'global': true,
        'lockfile-dir': '/home/src',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, 'Configuration conflict. "lockfile-dir" may not be used with "global"')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL')
    t.end()
  }
})

test('throw error if --hoist-pattern is used with --global', async (t) => {
  try {
    await getConfig({
      cliOptions: {
        'global': true,
        'hoist-pattern': 'eslint',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, 'Configuration conflict. "hoist-pattern" may not be used with "global"')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL')
    t.end()
  }
})

test('throw error if --virtual-store-dir is used with --global', async (t) => {
  try {
    await getConfig({
      cliOptions: {
        'global': true,
        'virtual-store-dir': 'pkgs',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, 'Configuration conflict. "virtual-store-dir" may not be used with "global"')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL')
    t.end()
  }
})

test('when using --global, link-workspace-packages, shared-workspace-shrinwrap and lockfile-directory are false even if it is set to true in a .npmrc file', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

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
        'global': false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.ok(config.linkWorkspacePackages)
    t.ok(config.sharedWorkspaceLockfile)
    t.ok(config.lockfileDir)
  }

  {
    const { config } = await getConfig({
      cliOptions: {
        'global': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.notOk(config.linkWorkspacePackages, 'link-workspace-packages is false')
    t.notOk(config.sharedWorkspaceLockfile, 'shared-workspace-lockfile is false')
    t.notOk(config.lockfileDir, 'lockfile-dir is null')
  }

  t.end()
})

test('registries of scoped packages are read', async (t) => {
  const { config } = await getConfig({
    cliOptions: {
      'dir': 'workspace',
      'userconfig': path.join(__dirname, 'scoped-registries.ini'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  // tslint:disable
  t.deepEqual(config.registries, {
    'default': 'https://default.com/',
    '@foo': 'https://foo.com/',
    '@bar': 'https://bar.com/',
  })
  // tslint:enable

  t.end()
})

test('registries in current directory\'s .npmrc have bigger priority then global config settings', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

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

  // tslint:disable
  t.deepEqual(config.registries, {
    'default': 'https://pnpm.js.org/',
    '@foo': 'https://foo.com/',
    '@bar': 'https://bar.com/',
  })
  // tslint:enable

  t.end()
})

test('filter is read from .npmrc as an array', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

  process.chdir(tmp)
  await fs.writeFile('.npmrc', 'filter=foo bar...', 'utf8')
  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  const { config } = await getConfig({
    cliOptions: {
      'global': false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  t.deepEqual(config.filter, ['foo', 'bar...'])

  t.end()
})

test('throw error if --save-prod is used with --save-peer', async (t) => {
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
    t.equal(err.message, 'A package cannot be a peer dependency and a prod dependency at the same time')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_PEER_CANNOT_BE_PROD_DEP')
    t.end()
  }
})

test('throw error if --save-optional is used with --save-peer', async (t) => {
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
    t.equal(err.message, 'A package cannot be a peer dependency and an optional dependency at the same time')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_PEER_CANNOT_BE_OPTIONAL_DEP')
    t.end()
  }
})

test('extraBinPaths', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

  process.chdir(tmp)

  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.deepEqual(config.extraBinPaths, [], 'extraBinPaths is empty outside of a workspace')
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
    t.deepEqual(config.extraBinPaths, [path.resolve('node_modules/.bin')], 'extraBinPaths has the node_modules/.bin folder from the root of the workspace')
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
    t.deepEqual(config.extraBinPaths, [], 'extraBinPaths is empty inside a workspace if scripts are ignored')
  }

  t.end()
})

test('convert shamefully-flatten to hoist-pattern=* and warn', async (t) => {
  const { config, warnings } = await getConfig({
    cliOptions: {
      'shamefully-flatten': true,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.deepEqual(config.hoistPattern, ['*'])
  t.equal(config.shamefullyHoist, true)
  t.deepEqual(warnings, [
    'The "shamefully-flatten" setting has been renamed to "shamefully-hoist". ' +
    'Also, in most cases you won\'t need "shamefully-hoist". ' +
    'Since v4, a semistrict node_modules structure is on by default (via hoist-pattern=[*]).',
  ])
  t.end()
})

test('hoist-pattern is undefined if --no-hoist used', async (t) => {
  const { config } = await getConfig({
    cliOptions: {
      'hoist': false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.equal(config.hoistPattern, undefined)
  t.end()
})

test('throw error if --no-hoist is used with --shamefully-hoist', async (t) => {
  try {
    await getConfig({
      cliOptions: {
        'hoist': false,
        'shamefully-hoist': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, '--shamefully-hoist cannot be used with --no-hoist')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_HOIST')
    t.end()
  }
})

test('throw error if --no-hoist is used with --shamefully-flatten', async (t) => {
  try {
    await getConfig({
      cliOptions: {
        'hoist': false,
        'shamefully-flatten': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, '--shamefully-flatten cannot be used with --no-hoist')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_HOIST')
    t.end()
  }
})

test('throw error if --no-hoist is used with --hoist-pattern', async (t) => {
  try {
    await getConfig({
      cliOptions: {
        'hoist': false,
        'hoist-pattern': 'eslint-*',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, '--hoist-pattern cannot be used with --no-hoist')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_HOIST')
    t.end()
  }
})

test('rawLocalConfig in a workspace', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

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

    t.deepEqual(config.rawLocalConfig, {
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

    t.deepEqual(config.rawLocalConfig, {
      'hoist-pattern': '*',
      'save-exact': true,
    })
  }
  t.end()
})

test('rawLocalConfig', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

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

  t.deepEqual(config.rawLocalConfig, {
    'modules-dir': 'modules',
    'save-exact': true,
  })
  t.end()
})

test('normalize the value of the color flag', async (t) => {
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

    t.equal(config.color, 'always')
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

    t.equal(config.color, 'never')
  }
  t.end()
})

test('read only supported settings from config', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

  process.chdir(tmp)
  await fs.writeFile('.npmrc', 'store-dir=__store__\nfoo=bar', 'utf8')

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.equal(config.storeDir, '__store__')
  t.equal(typeof config['foo'], 'undefined')
  t.equal(config.rawConfig['foo'], 'bar')

  t.end()
})

test('all CLI options are added to the config', async (t) => {
  const { config } = await getConfig({
    cliOptions: {
      'foo-bar': 'qar',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.equal(config['fooBar'], 'qar')
  t.end()
})

test('local prefix search stops on pnpm-workspace.yaml', async (t) => {
  const workspaceDir = path.join(__dirname, 'has-workspace-yaml')
  process.chdir(workspaceDir)
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.equal(config.dir, workspaceDir)
  t.end()
})
