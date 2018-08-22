import getConfigs from '@pnpm/config'
import test = require('tape')
import tempy = require('tempy')
import fs = require('mz/fs')

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

test('when using --global, link-workspace-packages is false even if it is set to true in a .npmrc file', async (t) => {
  const tmp = tempy.directory()
  t.comment(`temp dir created: ${tmp}`)

  process.chdir(tmp)
  fs.writeFile('.npmrc', 'link-workspace-packages=true', 'utf8')

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
    t.notOk(opts.linkWorkspacePackages)
  }

  t.end()
})
