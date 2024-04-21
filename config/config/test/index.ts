/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'fs'
import path from 'path'
import PATH from 'path-name'
import { getCurrentBranch } from '@pnpm/git-utils'
import { getConfig } from '@pnpm/config'
import { type PnpmError } from '@pnpm/error'
import loadNpmConf from '@pnpm/npm-conf'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'

import symlinkDir from 'symlink-dir'

jest.mock('@pnpm/git-utils', () => ({ getCurrentBranch: jest.fn() }))

// To override any local settings,
// we force the default values of config
delete process.env.npm_config_depth
process.env['npm_config_hoist'] = 'true'
delete process.env.npm_config_registry
delete process.env.npm_config_virtual_store_dir
delete process.env.npm_config_shared_workspace_lockfile
delete process.env.npm_config_side_effects_cache
delete process.env.npm_config_node_version

const env = {
  PNPM_HOME: __dirname,
  [PATH]: __dirname,
}
const f = fixtures(__dirname)

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
  // nodeVersion should not have a default value.
  // When not specified, the package-is-installable package detects nodeVersion automatically.
  expect(config.nodeVersion).toBeUndefined()
})

test('throw error if --link-workspace-packages is used with --global', async () => {
  try {
    await getConfig({
      cliOptions: {
        global: true,
        'link-workspace-packages': true,
      },
      env,
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err: any) { // eslint-disable-line
    expect(err.message).toEqual('Configuration conflict. "link-workspace-packages" may not be used with "global"')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL')
  }
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
  try {
    await getConfig({
      cliOptions: {
        global: true,
        'shared-workspace-lockfile': true,
      },
      env,
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err: any) { // eslint-disable-line
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
      env,
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err: any) { // eslint-disable-line
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
      env,
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err: any) { // eslint-disable-line
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
      env,
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  } catch (err: any) { // eslint-disable-line
    expect(err.message).toEqual('Configuration conflict. "virtual-store-dir" may not be used with "global"')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL')
  }
})

test('when using --global, link-workspace-packages, shared-workspace-lockfile and lockfile-dir are false even if it is set to true in a .npmrc file', async () => {
  prepareEmpty()

  const npmrc = [
    'link-workspace-packages=true',
    'shared-workspace-lockfile=true',
    'lockfile-dir=/home/src',
  ].join('\n')
  fs.writeFileSync('.npmrc', npmrc, 'utf8')
  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')

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
      env,
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

test('registries of scoped packages are read and normalized', async () => {
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
    default: 'https://default.com/',
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
      userconfig: path.join(__dirname, 'scoped-registries.ini'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.registries).toStrictEqual({
    default: 'https://pnpm.io/',
    '@foo': 'https://foo.com/',
    '@bar': 'https://bar.com/',
    '@qar': 'https://qar.com/qar',
  })
})

test('filter is read from .npmrc as an array', async () => {
  prepareEmpty()

  fs.writeFileSync('.npmrc', 'filter=foo bar...', 'utf8')
  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')

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

test('filter-prod is read from .npmrc as an array', async () => {
  prepareEmpty()

  fs.writeFileSync('.npmrc', 'filter-prod=foo bar...', 'utf8')
  fs.writeFileSync('pnpm-workspace.yaml', '', 'utf8')

  const { config } = await getConfig({
    cliOptions: {
      global: false,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.filterProd).toStrictEqual(['foo', 'bar...'])
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
  } catch (err: any) { // eslint-disable-line
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
  } catch (err: any) { // eslint-disable-line
    expect(err.message).toEqual('A package cannot be a peer dependency and an optional dependency at the same time')
    expect((err as PnpmError).code).toEqual('ERR_PNPM_CONFIG_CONFLICT_PEER_CANNOT_BE_OPTIONAL_DEP')
  }
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
  } catch (err: any) { // eslint-disable-line
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
  } catch (err: any) { // eslint-disable-line
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
  } catch (err: any) { // eslint-disable-line
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
  prepare()

  fs.writeFileSync('.npmrc', 'store-dir=__store__\nfoo=bar', 'utf8')

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.storeDir).toEqual('__store__')
  // @ts-expect-error
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

  // @ts-expect-error
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

test('respects test-pattern', async () => {
  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.testPattern).toBeUndefined()
  }
  {
    const workspaceDir = path.join(__dirname, 'using-test-pattern')
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
})

test('respects changed-files-ignore-pattern', async () => {
  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.changedFilesIgnorePattern).toBeUndefined()
  }
  {
    prepareEmpty()

    const npmrc = [
      'changed-files-ignore-pattern[]=.github/**',
      'changed-files-ignore-pattern[]=**/README.md',
    ].join('\n')

    fs.writeFileSync('.npmrc', npmrc, 'utf8')

    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
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
  expect(config.registry).toEqual('https://project-local.example.test')
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
      cafile: path.join(__dirname, 'cafile.txt'),
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

test('respect merge-git-branch-lockfiles-branch-pattern', async () => {
  {
    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.mergeGitBranchLockfilesBranchPattern).toBeUndefined()
    expect(config.mergeGitBranchLockfiles).toBeUndefined()
  }
  {
    prepareEmpty()

    const npmrc = [
      'merge-git-branch-lockfiles-branch-pattern[]=main',
      'merge-git-branch-lockfiles-branch-pattern[]=release/**',
    ].join('\n')

    fs.writeFileSync('.npmrc', npmrc, 'utf8')

    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.mergeGitBranchLockfilesBranchPattern).toEqual(['main', 'release/**'])
  }
})

test('getConfig() sets merge-git-branch-lockfiles when branch matches merge-git-branch-lockfiles-branch-pattern', async () => {
  prepareEmpty()
  {
    const npmrc = [
      'merge-git-branch-lockfiles-branch-pattern[]=main',
      'merge-git-branch-lockfiles-branch-pattern[]=release/**',
    ].join('\n')

    fs.writeFileSync('.npmrc', npmrc, 'utf8')

    ;(getCurrentBranch as jest.Mock).mockReturnValue('develop')
    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.mergeGitBranchLockfilesBranchPattern).toEqual(['main', 'release/**'])
    expect(config.mergeGitBranchLockfiles).toBe(false)
  }
  {
    (getCurrentBranch as jest.Mock).mockReturnValue('main')
    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    expect(config.mergeGitBranchLockfiles).toBe(true)
  }
  {
    (getCurrentBranch as jest.Mock).mockReturnValue('release/1.0.0')
    const { config } = await getConfig({
      cliOptions: {
        global: false,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
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
  fs.writeFileSync('.npmrc', 'foo=${ENV_VAR_123}', 'utf8') // eslint-disable-line
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
