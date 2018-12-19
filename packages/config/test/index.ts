import getConfigs from '@pnpm/config'
import fs = require('mz/fs')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')

test('getConfigs()', async (t) => {
  const configs = await getConfigs({
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
    t.equal(err['code'], 'ERR_PNPM_CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL')
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
    t.equal(err.message, 'Configuration conflict. "shared-workspace-shrinkwrap" may not be used with "global"')
    t.equal(err['code'], 'ERR_PNPM_CONFIG_CONFLICT_SHARED_WORKSPACE_SHRINKWRAP_WITH_GLOBAL')
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
    t.equal(err.message, 'Configuration conflict. "shrinkwrap-directory" may not be used with "global"')
    t.equal(err['code'], 'ERR_PNPM_CONFIG_CONFLICT_SHRINKWRAP_DIRECTORY_WITH_GLOBAL')
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
    const opts = await getConfigs({
      cliArgs: {
        'global': false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.ok(opts.linkWorkspacePackages)
    t.ok(opts.sharedWorkspaceShrinkwrap)
    t.ok(opts.shrinkwrapDirectory)
  }

  {
    const opts = await getConfigs({
      cliArgs: {
        'global': true,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    t.notOk(opts.linkWorkspacePackages, 'link-workspace-packages is false')
    t.notOk(opts.sharedWorkspaceShrinkwrap, 'shared-workspace-shrinkwrap is false')
    t.notOk(opts.shrinkwrapDirectory, 'shrinkwrap-directory is null')
  }

  t.end()
})

test('workspace manifest is searched from specified prefix', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

  process.chdir(tmp)

  await fs.mkdir('workspace')
  await fs.writeFile('workspace/pnpm-workspace.yaml', '', 'utf8')

  const opts = await getConfigs({
    cliArgs: {
      prefix: 'workspace',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.equal(opts.workspacePrefix, path.join(tmp, 'workspace'))
  t.end()
})

test('registries of scoped packages are read', async (t) => {
  const opts = await getConfigs({
    cliArgs: {
      prefix: 'workspace',
      userconfig: path.join(__dirname, 'scoped-registries.ini'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  t.deepEqual(opts.registries, {
    '@bar': 'https://bar.com',
    '@foo': 'https://foo.com',
    'default': 'https://registry.npmjs.org/',
  })

  t.end()
})

test('filter is read from .npmrc as an array', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

  process.chdir(tmp)
  await fs.writeFile('.npmrc', 'filter=foo bar...', 'utf8')
  await fs.writeFile('pnpm-workspace.yaml', '', 'utf8')

  const opts = await getConfigs({
    cliArgs: {
      'global': false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  t.deepEqual(opts.filter, ['foo', 'bar...'])

  t.end()
})

test('--side-effects-cache and --side-effects-cache-readonly', async (t) => {
  {
    const configs = await getConfigs({
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
    const configs = await getConfigs({
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
