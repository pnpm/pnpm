///<reference path="../../../typings/index.d.ts"/>
import getConfigs from '@pnpm/config'
import PnpmError from '@pnpm/error'
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')

import './findBestGlobalPrefixOnWindows'

delete process.env['npm_config_depth']

test('getConfigs()', async (t) => {
  const { configs } = await getConfigs({
    cliArgs: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  t.ok(configs)
  t.equal(configs.fetchRetries, 2)
  t.equal(configs.fetchRetryFactor, 10)
  t.equal(configs.fetchRetryMintimeout, 10000)
  t.equal(configs.fetchRetryMaxtimeout, 60000)
  t.end()
})

test('throw error if --link-workspace-packages is used with --global', async (t) => {
  try {
    await getConfigs({
      cliArgs: {
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

test('throw error if --shared-workspace-shrinkwrap is used with --global', async (t) => {
  try {
    await getConfigs({
      cliArgs: {
        'global': true,
        'shared-workspace-shrinkwrap': true,
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

test('throw error if --shrinkwrap-directory is used with --global', async (t) => {
  try {
    await getConfigs({
      cliArgs: {
        'global': true,
        'shrinkwrap-directory': '/home/src',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, 'Configuration conflict. "lockfile-directory" may not be used with "global"')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_DIRECTORY_WITH_GLOBAL')
    t.end()
  }
})

test('throw error if --independent-leaves is used with --global', async (t) => {
  try {
    await getConfigs({
      cliArgs: {
        'global': true,
        'independent-leaves': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, 'Configuration conflict. "independent-leaves" may not be used with "global"')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_CONFLICT_INDEPENDENT_LEAVES_WITH_GLOBAL')
    t.end()
  }
})

test('throw error if --hoist-pattern is used with --global', async (t) => {
  try {
    await getConfigs({
      cliArgs: {
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

test('when using --global, link-workspace-packages, shared-workspace-shrinwrap and shrinkwrap-directory are false even if it is set to true in a .npmrc file', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

  process.chdir(tmp)
  const npmrc = [
    'link-workspace-packages=true',
    'shared-workspace-shrinkwrap=true',
    'shrinkwrap-directory=/home/src',
  ].join('\n')
  await fs.writeFile('.npmrc', npmrc, 'utf8')
  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  {
    const { configs } = await getConfigs({
      cliArgs: {
        'global': false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.ok(configs.linkWorkspacePackages)
    t.ok(configs.sharedWorkspaceLockfile)
    t.ok(configs.lockfileDirectory)
  }

  {
    const { configs } = await getConfigs({
      cliArgs: {
        'global': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.notOk(configs.linkWorkspacePackages, 'link-workspace-packages is false')
    t.notOk(configs.sharedWorkspaceLockfile, 'shared-workspace-shrinkwrap is false')
    t.notOk(configs.lockfileDirectory, 'shrinkwrap-directory is null')
  }

  t.end()
})

test('workspace manifest is searched from specified prefix', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

  process.chdir(tmp)

  await fs.mkdir('workspace')
  await fs.writeFile('workspace/pnpm-workspace.yaml', '', 'utf8')

  const { configs } = await getConfigs({
    cliArgs: {
      prefix: 'workspace',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.equal(configs.workspacePrefix, path.join(tmp, 'workspace'))
  t.end()
})

test('registries of scoped packages are read', async (t) => {
  const { configs } = await getConfigs({
    cliArgs: {
      prefix: 'workspace',
      userconfig: path.join(__dirname, 'scoped-registries.ini'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  // tslint:disable
  t.deepEqual(configs.registries, {
    'default': 'https://default.com/',
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

  const { configs } = await getConfigs({
    cliArgs: {
      'global': false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  t.deepEqual(configs.filter, ['foo', 'bar...'])

  t.end()
})

test('--side-effects-cache and --side-effects-cache-readonly', async (t) => {
  {
    const { configs } = await getConfigs({
      cliArgs: {
        'side-effects-cache': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.ok(configs)
    t.ok(configs.sideEffectsCache) // for backward compatibility
    t.ok(configs.sideEffectsCacheRead)
    t.ok(configs.sideEffectsCacheWrite)
  }

  {
    const { configs } = await getConfigs({
      cliArgs: {
        'side-effects-cache-readonly': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.ok(configs)
    t.ok(configs.sideEffectsCacheReadonly) // for backward compatibility
    t.ok(configs.sideEffectsCacheRead)
    t.notOk(configs.sideEffectsCacheWrite)
  }

  t.end()
})

test('depth is 0 by default for list commands', async (t) => {
  {
    const { configs } = await getConfigs({
      cliArgs: {},
      command: ['list'],
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.equal(configs.depth, 0)
  }

  {
    const { configs } = await getConfigs({
      cliArgs: {},
      command: ['list', 'bole'],
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.equal(configs.depth, 0)
  }

  {
    const { configs } = await getConfigs({
      cliArgs: {},
      command: ['recursive', 'list'],
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.equal(configs.depth, 0)
  }

  {
    const { configs } = await getConfigs({
      cliArgs: {},
      command: ['recursive', 'list', 'bole'],
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.equal(configs.depth, 0)
  }

  {
    const { configs } = await getConfigs({
      cliArgs: {},
      command: ['recursive', 'list'],
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.equal(configs.depth, 0)
  }

  {
    const { configs } = await getConfigs({
      cliArgs: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.equal(configs.depth, Infinity)
  }

  t.end()
})

test('when runnning a global command inside a workspace, the workspace should be ignored', async (t) => {
  {
    const { configs } = await getConfigs({
      cliArgs: {
        'global': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.ok(configs)
    t.notOk(configs.workspacePrefix)
  }

  {
    const { configs } = await getConfigs({
      cliArgs: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.ok(configs)
    t.ok(configs.workspacePrefix)
  }

  t.end()
})

test('throw error if --save-prod is used with --save-peer', async (t) => {
  try {
    await getConfigs({
      cliArgs: {
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
    await getConfigs({
      cliArgs: {
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
    const { configs } = await getConfigs({
      cliArgs: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.deepEqual(configs.extraBinPaths, [], 'extraBinPaths is empty outside of a workspace')
  }

  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  {
    const { configs } = await getConfigs({
      cliArgs: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.deepEqual(configs.extraBinPaths, [path.resolve('node_modules/.bin')], 'extraBinPaths has the node_modules/.bin folder from the root of the workspace')
  }

  {
    const { configs } = await getConfigs({
      cliArgs: {
        'ignore-scripts': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.deepEqual(configs.extraBinPaths, [], 'extraBinPaths is empty inside a workspace if scripts are ignored')
  }

  t.end()
})

test('throw error on "update --frozen-lockfile"', async (t) => {
  try {
    await getConfigs({
      cliArgs: {
        'frozen-lockfile': true,
      },
      command: ['update'],
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, 'The "frozen-lockfile" option cannot be used with the "update" command')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_BAD_OPTION')
    t.end()
  }
})

test('throw error on "update --prefer-frozen-lockfile"', async (t) => {
  try {
    await getConfigs({
      cliArgs: {
        'prefer-frozen-lockfile': true,
      },
      command: ['update'],
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err) {
    t.equal(err.message, 'The "prefer-frozen-lockfile" option cannot be used with the "update" command')
    t.equal((err as PnpmError).code, 'ERR_PNPM_CONFIG_BAD_OPTION')
    t.end()
  }
})

test('convert shamefully-flatten to hoist-pattern=* and warn', async (t) => {
  const { configs, warnings } = await getConfigs({
    cliArgs: {
      'shamefully-flatten': true,
    },
    command: ['install'],
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.equal(configs.hoistPattern, '*')
  t.deepEqual(warnings, ['The "shamefully-flatten" setting is deprecated. ' +
    'Use "shamefully-hoist", "hoist" or "hoist-pattern" instead. ' +
    'Since v4, hoisting is on by default for all dependencies.'])
  t.end()
})

test('hoist-pattern is undefined if --no-hoist used', async (t) => {
  const { configs } = await getConfigs({
    cliArgs: {
      'hoist': false,
    },
    command: ['install'],
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.equal(configs.hoistPattern, undefined)
  t.end()
})
