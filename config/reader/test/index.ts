/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, jest, test } from '@jest/globals'
import { GLOBAL_LAYOUT_VERSION } from '@pnpm/constants'
import { prepare, prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import PATH from 'path-name'
import { symlinkDir } from 'symlink-dir'
import { writeYamlFileSync } from 'write-yaml-file'

jest.unstable_mockModule('@pnpm/network.git-utils', () => ({ getCurrentBranch: jest.fn() }))

const { getConfig, parsePackageManager } = await import('@pnpm/config.reader')
const { getCurrentBranch } = await import('@pnpm/network.git-utils')

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
  [PATH]: path.join(import.meta.dirname, 'bin'),
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

test.each([
  { field: 'devEngines' as const, version: '22.20.0', onFail: 'download' as const, expected: '22.20.0' },
  { field: 'devEngines' as const, version: '22.20.0', onFail: 'error' as const, expected: '22.20.0' },
  { field: 'devEngines' as const, version: '^22.0.0', onFail: 'download' as const, expected: '22.0.0' },
  { field: 'engines' as const, version: '22.20.0', onFail: 'download' as const, expected: '22.20.0' },
])('when $field is $version and onFail is $onFail, nodeVersion is set to $expected', async ({ field, version, onFail, expected }) => {
  prepare({
    [field]: {
      runtime: {
        name: 'node',
        version,
        onFail,
      },
    },
  })

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.nodeVersion).toBe(expected)
})

test('nodeVersion from config takes priority over devEngines.runtime', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '22.20.0',
        onFail: 'download',
      },
    },
  })

  const { config } = await getConfig({
    cliOptions: {
      'node-version': '20.0.0',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.nodeVersion).toBe('20.0.0')
})

test('runtimeOnFail=download overrides devEngines.runtime.onFail and adds node to devDependencies', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '22.20.0',
      },
    },
  })

  const { config, context } = await getConfig({
    cliOptions: {
      'runtime-on-fail': 'download',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.runtimeOnFail).toBe('download')
  const runtime = context.rootProjectManifest?.devEngines?.runtime
  expect(Array.isArray(runtime) ? runtime[0] : runtime).toMatchObject({
    name: 'node',
    onFail: 'download',
  })
  expect(context.rootProjectManifest?.devDependencies?.node).toBe('runtime:22.20.0')
})

test('runtimeOnFail=ignore overrides an existing onFail=download and removes node from devDependencies', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '22.20.0',
        onFail: 'download',
      },
    },
  })

  const { config, context } = await getConfig({
    cliOptions: {
      'runtime-on-fail': 'ignore',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.runtimeOnFail).toBe('ignore')
  const runtime = context.rootProjectManifest?.devEngines?.runtime
  expect(Array.isArray(runtime) ? runtime[0] : runtime).toMatchObject({
    name: 'node',
    onFail: 'ignore',
  })
  expect(context.rootProjectManifest?.devDependencies?.node).toBeUndefined()
})

test('devEngines.packageManager without onFail resolves to the documented pmOnFail default "download" (#11676)', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '11.0.0',
      },
    },
  })

  const { context } = await getConfig({
    cliOptions: {},
    packageManager: { name: 'pnpm', version: '11.0.0' },
  })

  expect(context.wantedPackageManager).toMatchObject({
    name: 'pnpm',
    version: '11.0.0',
    onFail: 'download',
  })
})

test('devEngines.packageManager with explicit onFail is respected (regression guard for #11676)', async () => {
  prepare({
    devEngines: {
      packageManager: {
        name: 'pnpm',
        version: '11.0.0',
        onFail: 'error',
      },
    },
  })

  const { context } = await getConfig({
    cliOptions: {},
    packageManager: { name: 'pnpm', version: '11.0.0' },
  })

  expect(context.wantedPackageManager?.onFail).toBe('error')
})

describe('"packageManager" / "devEngines.packageManager" conflict warning', () => {
  const HASH_A = 'sha512.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
  const HASH_B = 'sha512.bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
  const IGNORED = '. "packageManager" will be ignored'

  async function warningsFor (packageManager: string, devEnginesVersion: string): Promise<string[]> {
    prepare({
      packageManager,
      devEngines: {
        packageManager: { name: 'pnpm', version: devEnginesVersion, onFail: 'ignore' },
      },
    })
    const { warnings } = await getConfig({
      cliOptions: {},
      packageManager: { name: 'pnpm', version: '1.2.3' },
    })
    return warnings
  }

  test.each([
    { caseName: 'same version without a hash', packageManager: 'pnpm@1.2.3', devEnginesVersion: '1.2.3' },
    { caseName: 'same version with the same hash', packageManager: `pnpm@1.2.3+${HASH_A}`, devEnginesVersion: `1.2.3+${HASH_A}` },
  ])('does not warn when the specifiers are identical: $caseName', async ({ packageManager, devEnginesVersion }) => {
    const warnings = await warningsFor(packageManager, devEnginesVersion)
    expect(warnings.some(w => w.includes(IGNORED))).toBe(false)
  })

  test.each([
    { caseName: 'hash on the legacy field only', packageManager: `pnpm@1.2.3+${HASH_A}`, devEnginesVersion: '1.2.3' },
    { caseName: 'hash on the devEngines field only', packageManager: 'pnpm@1.2.3', devEnginesVersion: `1.2.3+${HASH_A}` },
  ])('warns generically when the integrity hash is present on only one field: $caseName', async ({ packageManager, devEnginesVersion }) => {
    const warnings = await warningsFor(packageManager, devEnginesVersion)
    expect(warnings).toContain(`Cannot use both "packageManager" and "devEngines.packageManager" in package.json${IGNORED}`)
  })

  test('warns about contradictory integrity hashes for the same version', async () => {
    const warnings = await warningsFor(`pnpm@1.2.3+${HASH_A}`, `1.2.3+${HASH_B}`)
    expect(warnings).toContain(`"packageManager" and "devEngines.packageManager" specify pnpm@1.2.3 with different integrity hashes in package.json${IGNORED}`)
  })

  test.each([
    { caseName: 'the legacy field is a URL reference', packageManager: 'pnpm@https://github.com/pnpm/pnpm' },
    { caseName: 'the legacy field is a bare name with no version', packageManager: 'pnpm' },
  ])('warns generically rather than claiming a version mismatch when one side is not a concrete version: $caseName', async ({ packageManager }) => {
    const warnings = await warningsFor(packageManager, '1.2.3')
    expect(warnings).toContain(`Cannot use both "packageManager" and "devEngines.packageManager" in package.json${IGNORED}`)
    expect(warnings.some(w => w.includes('different versions'))).toBe(false)
  })

  test.each([
    { caseName: 'different exact versions', devEnginesVersion: '1.2.4' },
    { caseName: 'exact version versus a range', devEnginesVersion: '>=1.0.0' },
  ])('warns about a version mismatch: $caseName', async ({ devEnginesVersion }) => {
    const warnings = await warningsFor('pnpm@1.2.3', devEnginesVersion)
    expect(warnings).toContain(`"packageManager" and "devEngines.packageManager" specify different versions of pnpm in package.json${IGNORED}`)
  })

  test('warns when the fields name different package managers', async () => {
    prepare({
      packageManager: 'yarn@1.2.3',
      devEngines: {
        packageManager: { name: 'pnpm', version: '1.2.3', onFail: 'ignore' },
      },
    })
    const { warnings } = await getConfig({
      cliOptions: {},
      packageManager: { name: 'pnpm', version: '1.2.3' },
    })
    expect(warnings).toContain(`"packageManager" (yarn) and "devEngines.packageManager" (pnpm) specify different package managers in package.json${IGNORED}`)
  })

  test('strips control characters from package.json values embedded in the warning', async () => {
    prepare({
      packageManager: 'ev\u001b[31mi\nl@1.2.3',
      devEngines: {
        packageManager: { name: 'pnpm', version: '1.2.3', onFail: 'ignore' },
      },
    })
    const { warnings } = await getConfig({
      cliOptions: {},
      packageManager: { name: 'pnpm', version: '1.2.3' },
    })
    const warning = warnings.find(w => w.includes('different package managers'))
    expect(warning).toBeDefined()
    // eslint-disable-next-line no-control-regex
    expect(warning).not.toMatch(/[\u0000-\u001f\u007f]/)
    expect(warning).toContain('"packageManager" (evi l)')
  })
})

describe('parsePackageManager', () => {
  test.each([
    { input: 'pnpm@9.5.0', expected: { name: 'pnpm', version: '9.5.0', hash: undefined } },
    { input: 'pnpm@9.5.0+sha512.abc123', expected: { name: 'pnpm', version: '9.5.0', hash: 'sha512.abc123' } },
    { input: 'pnpm@9.5.0+a+b', expected: { name: 'pnpm', version: '9.5.0', hash: 'a+b' } },
    { input: 'pnpm', expected: { name: 'pnpm', version: undefined, hash: undefined } },
    { input: '@scope/pm@1.2.3', expected: { name: '@scope/pm', version: '1.2.3', hash: undefined } },
    { input: 'pnpm@https://github.com/pnpm/pnpm', expected: { name: 'pnpm', version: undefined, hash: undefined } },
  ])('parses $input', ({ input, expected }) => {
    expect(parsePackageManager(input)).toEqual(expected)
  })
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

  // rc options appear as usual. Unscoped credentials (`username`,
  // `_authToken`) are rescoped to the file's registry at load — the .npmrc
  // here doesn't set its own `registry=`, so they pin to the npmjs default.
  expect(config.authConfig).toMatchObject({
    '//my-org.registry.example.com:username': 'some-employee',
    '//my-org.registry.example.com:_authToken': 'some-employee-token',
    '@my-org:registry': 'https://my-org.registry.example.com',
    '@jsr:registry': 'https://not-actually-jsr.example.com',
    '//registry.npmjs.org/:username': 'example-user-name',
    '//registry.npmjs.org/:_authToken': 'example-auth-token',
  })
  expect(config.authConfig.username).toBeUndefined()
  expect(config.authConfig._authToken).toBeUndefined()

  // workspace-specific settings are omitted
  expect(config.authConfig['dlx-cache-max-age']).toBeUndefined()
  expect(config.authConfig['dlxCacheMaxAge']).toBeUndefined()
  expect(config.dlxCacheMaxAge).toBe(24 * 60) // TODO: refactor to make defaultOptions importable
  expect(config.authConfig['trust-policy-exclude']).toBeUndefined()
  expect(config.authConfig['trustPolicyExclude']).toBeUndefined()
  expect(config.trustPolicyExclude).toBeUndefined()
  expect(config.authConfig.packages).toBeUndefined()
})

describe('minimumReleaseAgeStrict default', () => {
  test('defaults to true when minimumReleaseAge is set in pnpm-workspace.yaml', async () => {
    prepareEmpty()

    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: 60,
    })

    const { config } = await getConfig({
      cliOptions: {},
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.minimumReleaseAge).toBe(60)
    expect(config.minimumReleaseAgeStrict).toBe(true)
  })

  test('defaults to true when minimumReleaseAge is set on the CLI', async () => {
    prepareEmpty()

    const { config } = await getConfig({
      cliOptions: {
        'minimum-release-age': 60,
      },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.minimumReleaseAge).toBe(60)
    expect(config.minimumReleaseAgeStrict).toBe(true)
  })

  test('defaults to true when minimumReleaseAge is set via pnpm_config_* env var', async () => {
    prepareEmpty()

    const { config } = await getConfig({
      cliOptions: {},
      env: {
        pnpm_config_minimum_release_age: '60',
      },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.minimumReleaseAge).toBe(60)
    expect(config.minimumReleaseAgeStrict).toBe(true)
  })

  test('respects an explicit minimumReleaseAgeStrict=false from pnpm-workspace.yaml', async () => {
    prepareEmpty()

    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: 60,
      minimumReleaseAgeStrict: false,
    })

    const { config } = await getConfig({
      cliOptions: {},
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.minimumReleaseAge).toBe(60)
    expect(config.minimumReleaseAgeStrict).toBe(false)
  })

  test('does not become strict when only the built-in default for minimumReleaseAge applies', async () => {
    prepareEmpty()

    writeYamlFileSync('pnpm-workspace.yaml', {})

    const { config } = await getConfig({
      cliOptions: {},
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.minimumReleaseAge).toBe(1440)
    expect(config.minimumReleaseAgeStrict).toBeUndefined()
  })
})

test('camelCase settings from pnpm-workspace.yaml are read into typed Config properties', async () => {
  prepareEmpty()

  writeYamlFileSync('pnpm-workspace.yaml', {
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
  })
})

test('workspace-specific settings are read into typed Config properties', async () => {
  prepareEmpty()

  writeYamlFileSync('pnpm-workspace.yaml', {
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

  expect(config.workspacePackagePatterns).toStrictEqual(['foo', 'bar'])
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

  writeYamlFileSync('pnpm-workspace.yaml', {
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
  expect(config.packageManagerRegistries?.default).toBe('https://default.com/')
})

test('project .npmrc does not expand env variables in registry URLs', async () => {
  prepare()

  fs.writeFileSync('.npmrc', 'registry=https://registry.example.com/${PNPM_TEST_TOKEN}/\n', 'utf8')

  const { config, warnings } = await getConfig({
    cliOptions: {},
    env: { ...env, PNPM_TEST_TOKEN: 'secret' },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.registries.default).not.toBe('https://registry.example.com/secret/')
  expect(JSON.stringify(config.registries)).not.toContain('secret')
  expect(warnings).toEqual(expect.arrayContaining([
    expect.stringContaining('Ignored project-level request destination "registry"'),
  ]))
  // The warning should guide the user toward a trusted source and the docs.
  const registryWarning = warnings.find((w) => w.includes('Ignored project-level request destination "registry"')) ?? ''
  expect(registryWarning).toContain('~/.npmrc')
  expect(registryWarning).toContain('pnpm config set "registry" <value>')
  expect(registryWarning).toContain('https://pnpm.io/npmrc')
})

test('project .npmrc does not expand env variables in scoped registry URLs or URL-scoped keys', async () => {
  prepare()

  fs.writeFileSync('.npmrc', [
    '@scope:registry=https://registry.example.com/${PNPM_TEST_TOKEN}/',
    '//registry.example.com/${PNPM_TEST_TOKEN}/:_authToken=token',
    '',
  ].join('\n'), 'utf8')

  const { config, warnings } = await getConfig({
    cliOptions: {},
    env: { ...env, PNPM_TEST_TOKEN: 'secret' },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.registries['@scope']).toBeUndefined()
  expect(Object.keys(config.authConfig).join('\n')).not.toContain('secret')
  expect(warnings).toEqual(expect.arrayContaining([
    expect.stringContaining('Ignored project-level request destination "@scope:registry"'),
    expect.stringContaining('Ignored project-level request destination "//registry.example.com/${PNPM_TEST_TOKEN}/:_authToken"'),
  ]))
  // When the key itself contains a ${...} placeholder, the warning must not
  // embed it in a runnable `pnpm config set "<key>"` command — a shell would
  // expand the placeholder on copy-paste.
  const urlScopedWarning = warnings.find((w) => w.includes('//registry.example.com/${PNPM_TEST_TOKEN}/:_authToken')) ?? ''
  expect(urlScopedWarning).not.toContain('pnpm config set "')
  expect(urlScopedWarning).toContain('~/.npmrc')
})

test('the warning never embeds a shell-unsafe key in a runnable pnpm config set command', async () => {
  prepare()

  // A malicious repository could craft a key with shell metacharacters; the
  // suggested copy-paste command must not become a command-injection vector.
  fs.writeFileSync('.npmrc', '//$(touch pwned)`id`/:_authToken=${PNPM_TEST_TOKEN}\n', 'utf8')

  const { warnings } = await getConfig({
    cliOptions: {},
    env: { ...env, PNPM_TEST_TOKEN: 'secret' },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  const unsafeWarning = warnings.find((w) => w.includes('$(touch pwned)')) ?? ''
  expect(unsafeWarning).not.toBe('')
  expect(unsafeWarning).not.toContain('pnpm config set "')
})

test('project .npmrc does not expand env variables in auth values', async () => {
  prepare()

  fs.writeFileSync('.npmrc', [
    'registry=https://attacker.example/',
    '//attacker.example/:_authToken=${PNPM_TEST_TOKEN}',
    '//attacker.example/:cert=${PNPM_TEST_CERT}',
    '//attacker.example/:key=${PNPM_TEST_KEY}',
    '_authToken=${PNPM_TEST_TOKEN}',
    'username=${PNPM_TEST_USER}',
    '_password=${PNPM_TEST_PASSWORD}',
    'cert=${PNPM_TEST_CERT}',
    'key=${PNPM_TEST_KEY}',
    '',
  ].join('\n'), 'utf8')

  const { config, warnings } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      PNPM_TEST_CERT: 'secret-cert',
      PNPM_TEST_KEY: 'secret-key',
      PNPM_TEST_PASSWORD: Buffer.from('secret').toString('base64'),
      PNPM_TEST_TOKEN: 'secret-token',
      PNPM_TEST_USER: 'secret-user',
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  const serializedAuthConfig = JSON.stringify(config.authConfig)
  expect(serializedAuthConfig).not.toContain('secret-token')
  expect(serializedAuthConfig).not.toContain('secret-user')
  expect(serializedAuthConfig).not.toContain('secret-cert')
  expect(serializedAuthConfig).not.toContain('secret-key')
  expect(config.configByUri?.['//attacker.example/']?.['@']).toBeUndefined()
  expect(config.configByUri?.['//attacker.example/']?.tls).toBeUndefined()
  expect(warnings).toEqual(expect.arrayContaining([
    expect.stringContaining('Ignored project-level auth setting "//attacker.example/:_authToken"'),
    expect.stringContaining('Ignored project-level auth setting "//attacker.example/:cert"'),
    expect.stringContaining('Ignored project-level auth setting "//attacker.example/:key"'),
    expect.stringContaining('Ignored project-level auth setting "_authToken"'),
    expect.stringContaining('Ignored project-level auth setting "cert"'),
    expect.stringContaining('Ignored project-level auth setting "key"'),
  ]))
  // The warning should tell the user how to migrate the credential.
  const authWarning = warnings.find((w) => w.includes('Ignored project-level auth setting "//attacker.example/:_authToken"')) ?? ''
  expect(authWarning).toContain('pnpm config set "//attacker.example/:_authToken" <value>')
  expect(authWarning).toContain('~/.npmrc')
  expect(authWarning).toContain('https://pnpm.io/npmrc')
})

test('project .npmrc does not expand env variables in proxy URLs', async () => {
  prepare()

  fs.writeFileSync('.npmrc', [
    'https-proxy=http://proxy.example.com/${PNPM_TEST_TOKEN}/',
    'http-proxy=http://proxy.example.com/${PNPM_TEST_TOKEN}/',
    'proxy=http://legacy-proxy.example.com/${PNPM_TEST_TOKEN}/',
    '',
  ].join('\n'), 'utf8')

  const { config, warnings } = await getConfig({
    cliOptions: {},
    env: { ...env, PNPM_TEST_TOKEN: 'secret' },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.httpsProxy).toBeUndefined()
  expect(config.httpProxy).toBeUndefined()
  expect(JSON.stringify(config)).not.toContain('secret')
  expect(warnings).toEqual(expect.arrayContaining([
    expect.stringContaining('Ignored project-level request destination "https-proxy"'),
    expect.stringContaining('Ignored project-level request destination "http-proxy"'),
    expect.stringContaining('Ignored project-level request destination "proxy"'),
  ]))
})

test('user .npmrc may expand env variables in registry URLs', async () => {
  prepare()

  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry=https://registry.example.com/${PNPM_TEST_TOKEN}/\n', 'utf8')

  const { config } = await getConfig({
    cliOptions: {
      userconfig: path.resolve('user-home', '.npmrc'),
    },
    env: { ...env, PNPM_TEST_TOKEN: 'secret' },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(config.registries.default).toBe('https://registry.example.com/secret/')
})

test('pnpm-workspace.yaml registries override the same scope from .npmrc (#11492)', async () => {
  prepareEmpty()

  fs.writeFileSync('.npmrc', '@my-org:registry=https://from-npmrc.example.com/', 'utf8')
  writeYamlFileSync('pnpm-workspace.yaml', {
    registries: {
      '@my-org': 'https://from-workspace-yaml.example.com/',
    },
  })

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: { name: 'pnpm', version: '1.0.0' },
    workspaceDir: process.cwd(),
  })

  expect(config.registries['@my-org']).toBe('https://from-workspace-yaml.example.com/')
})

test('pnpm-workspace.yaml registries.default is reflected in config.registry (#10099)', async () => {
  prepareEmpty()

  writeYamlFileSync('pnpm-workspace.yaml', {
    registries: {
      default: 'https://private.example.com/',
    },
  })

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: { name: 'pnpm', version: '1.0.0' },
    workspaceDir: process.cwd(),
  })

  expect(config.registry).toBe('https://private.example.com/')
  expect(config.registries.default).toBe('https://private.example.com/')
})

test('pnpm-workspace.yaml request destinations do not expand env variables', async () => {
  prepareEmpty()

  writeYamlFileSync('pnpm-workspace.yaml', {
    pnprServer: 'https://${PNPM_TEST_TOKEN}.evil.example/',
    registries: {
      default: 'https://private.example.com/${PNPM_TEST_TOKEN}/',
      '@scope': 'https://scope.example.com/${PNPM_TEST_TOKEN}/',
    },
    namedRegistries: {
      work: 'https://work.example.com/${PNPM_TEST_TOKEN}/',
    },
  })

  const { config } = await getConfig({
    cliOptions: {},
    env: { ...env, PNPM_TEST_TOKEN: 'secret' },
    packageManager: { name: 'pnpm', version: '1.0.0' },
    workspaceDir: process.cwd(),
  })

  expect(config.registries.default).not.toBe('https://private.example.com/secret/')
  expect(config.registries['@scope']).toBeUndefined()
  expect(config.namedRegistries).toStrictEqual({})
  expect(config.pnprServer).toBeUndefined()
  expect(JSON.stringify(config)).not.toContain('secret')
})

test('package manager bootstrap registries ignore project workspace registries', async () => {
  prepareEmpty()

  fs.writeFileSync('user.npmrc', [
    'registry=https://trusted.example.com/',
    '@pnpm:registry=https://trusted-pnpm.example.com/',
    'strict-ssl=true',
    '//trusted.example.com/:_authToken=trusted-token',
    '',
  ].join('\n'), 'utf8')
  fs.writeFileSync('.npmrc', [
    'registry=https://project.example.com/',
    'https-proxy=http://project-proxy.example.com:8080',
    'strict-ssl=false',
    '//project.example.com/:_authToken=project-token',
    '',
  ].join('\n'), 'utf8')
  writeYamlFileSync('pnpm-workspace.yaml', {
    registries: {
      '@pnpm': 'https://workspace-pnpm.example.com/',
      default: 'https://workspace.example.com/',
    },
  })

  const { config } = await getConfig({
    cliOptions: {
      userconfig: path.resolve('user.npmrc'),
    },
    env: {
      ...env,
      XDG_CONFIG_HOME: path.resolve('xdg-config'),
      https_proxy: 'http://trusted-env-proxy.example.com:8080',
      no_proxy: 'trusted-env-no-proxy.example.com',
    },
    packageManager: { name: 'pnpm', version: '1.0.0' },
    workspaceDir: process.cwd(),
  })

  expect(config.registries).toMatchObject({
    '@pnpm': 'https://workspace-pnpm.example.com/',
    default: 'https://workspace.example.com/',
  })
  expect(config.packageManagerRegistries).toMatchObject({
    '@pnpm': 'https://trusted-pnpm.example.com/',
    default: 'https://trusted.example.com/',
  })
  expect(config.httpsProxy).toBe('http://project-proxy.example.com:8080')
  expect(config.strictSsl).toBe(false)
  expect(config.configByUri).toMatchObject({
    '//project.example.com/': { '@': { authToken: 'project-token' } },
  })
  expect(config.packageManagerNetworkConfig).toMatchObject({
    configByUri: {
      '//trusted.example.com/': { '@': { authToken: 'trusted-token' } },
    },
    httpProxy: 'http://trusted-env-proxy.example.com:8080',
    httpsProxy: 'http://trusted-env-proxy.example.com:8080',
    noProxy: 'trusted-env-no-proxy.example.com',
    strictSsl: true,
  })
  expect(config.packageManagerNetworkConfig?.configByUri['//project.example.com/']).toBeUndefined()
})

test('CLI --registry overrides pnpm-workspace.yaml registries.default (#10099)', async () => {
  prepareEmpty()

  writeYamlFileSync('pnpm-workspace.yaml', {
    registries: {
      default: 'https://workspace.example.com/',
    },
  })

  const { config } = await getConfig({
    cliOptions: { registry: 'https://cli.example.com/' },
    packageManager: { name: 'pnpm', version: '1.0.0' },
    workspaceDir: process.cwd(),
  })

  expect(config.registry).toBe('https://cli.example.com/')
  expect(config.packageManagerRegistries?.default).toBe('https://cli.example.com/')
})

test('auth tokens from pnpm auth file override ~/.npmrc', async () => {
  prepareEmpty()

  // Set up a user .npmrc with a stale token
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), '//registry.npmjs.org/:_authToken=stale-token', 'utf8')

  // Set up a pnpm auth file with a fresh token via XDG_CONFIG_HOME
  const configHome = path.resolve('xdg-config')
  fs.mkdirSync(path.join(configHome, 'pnpm'), { recursive: true })
  fs.writeFileSync(
    path.join(configHome, 'pnpm', 'auth.ini'),
    '//registry.npmjs.org/:_authToken=fresh-token'
  )

  const originalXdg = process.env.XDG_CONFIG_HOME
  process.env.XDG_CONFIG_HOME = configHome
  try {
    const { config } = await getConfig({
      cliOptions: {
        userconfig: path.resolve('user-home', '.npmrc'),
      },
      env: {
        ...env,
        XDG_CONFIG_HOME: configHome,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.authConfig['//registry.npmjs.org/:_authToken']).toBe('fresh-token')
  } finally {
    if (originalXdg != null) {
      process.env.XDG_CONFIG_HOME = originalXdg
    } else {
      delete process.env.XDG_CONFIG_HOME
    }
  }
})

test('reads URL-scoped auth from npm_config_// environment variables', async () => {
  prepareEmpty()

  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      'npm_config_//env-test.example/:_authToken': 'npm-env-token',
    },
    packageManager: { name: 'pnpm', version: '1.0.0' },
  })

  expect(config.authConfig['//env-test.example/:_authToken']).toBe('npm-env-token')
})

test('reads URL-scoped auth from pnpm_config_// environment variables', async () => {
  prepareEmpty()

  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      'pnpm_config_//env-test.example/:_authToken': 'pnpm-env-token',
    },
    packageManager: { name: 'pnpm', version: '1.0.0' },
  })

  expect(config.authConfig['//env-test.example/:_authToken']).toBe('pnpm-env-token')
})

test('pnpm_config_// takes precedence over npm_config_// for the same key', async () => {
  prepareEmpty()

  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      'npm_config_//env-test.example/:_authToken': 'npm-env-token',
      'pnpm_config_//env-test.example/:_authToken': 'pnpm-env-token',
    },
    packageManager: { name: 'pnpm', version: '1.0.0' },
  })

  expect(config.authConfig['//env-test.example/:_authToken']).toBe('pnpm-env-token')
})

test('the npm_config_// / pnpm_config_// prefix is matched case-insensitively', async () => {
  prepareEmpty()

  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      // Upper-case npm prefix and mixed-case pnpm prefix; the latter (pnpm) wins.
      'NPM_CONFIG_//env-test.example/:_authToken': 'npm-upper-token',
      'PnPm_Config_//env-test.example/:_authToken': 'pnpm-mixed-token',
    },
    packageManager: { name: 'pnpm', version: '1.0.0' },
  })

  expect(config.authConfig['//env-test.example/:_authToken']).toBe('pnpm-mixed-token')
})

test('a tokenHelper set via a URL-scoped env var is not honored (no project-config error)', async () => {
  prepareEmpty()

  // tokenHelper executes a binary and is only valid from a user-level config
  // file; the env layer must drop it rather than trip the project-config guard.
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      'npm_config_//env-test.example/:tokenHelper': '/bin/echo',
    },
    packageManager: { name: 'pnpm', version: '1.0.0' },
  })

  expect(config.authConfig['//env-test.example/:tokenHelper']).toBeUndefined()
})

test('URL-scoped auth from the environment overrides a project .npmrc for the same host', async () => {
  prepareEmpty()

  // The repository ships a literal token for the host; the trusted env value must win.
  fs.writeFileSync('.npmrc', '//env-test.example/:_authToken=workspace-token', 'utf8')

  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      'npm_config_//env-test.example/:_authToken': 'env-token',
    },
    packageManager: { name: 'pnpm', version: '1.0.0' },
  })

  expect(config.authConfig['//env-test.example/:_authToken']).toBe('env-token')
})

test('a CLI-provided URL-scoped auth token overrides the same env var', async () => {
  prepareEmpty()

  // Precedence is workspace < env < CLI; an explicit CLI value must still win.
  const { config } = await getConfig({
    cliOptions: {
      '//env-test.example/:_authToken': 'cli-token',
    },
    env: {
      ...env,
      'npm_config_//env-test.example/:_authToken': 'env-token',
    },
    packageManager: { name: 'pnpm', version: '1.0.0' },
  })

  expect(config.authConfig['//env-test.example/:_authToken']).toBe('cli-token')
})

test('URL-scoped env vars honor non-token credential fields and ignore non-URL keys', async () => {
  prepareEmpty()

  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      'npm_config_//env-test.example/:username': 'env-user',
      'npm_config_//env-test.example/:_password': 'ZW52LXBhc3M=', // base64, value is opaque to pnpm
      // A non-URL-scoped key must not be imported by the //-scoped env reader.
      'npm_config_always-auth': 'true',
    },
    packageManager: { name: 'pnpm', version: '1.0.0' },
  })

  expect(config.authConfig['//env-test.example/:username']).toBe('env-user')
  expect(config.authConfig['//env-test.example/:_password']).toBe('ZW52LXBhc3M=')
  expect(config.authConfig['always-auth']).toBeUndefined()
})

test('workspace .npmrc overrides pnpm auth file', async () => {
  prepareEmpty()

  // Set up a workspace .npmrc with a project-specific token
  fs.writeFileSync('.npmrc', '//registry.npmjs.org/:_authToken=workspace-token', 'utf8')

  // Set up a pnpm auth file with a different token
  const configHome = path.resolve('xdg-config')
  fs.mkdirSync(path.join(configHome, 'pnpm'), { recursive: true })
  fs.writeFileSync(
    path.join(configHome, 'pnpm', 'auth.ini'),
    '//registry.npmjs.org/:_authToken=global-token'
  )

  const originalXdg = process.env.XDG_CONFIG_HOME
  process.env.XDG_CONFIG_HOME = configHome
  try {
    const { config } = await getConfig({
      cliOptions: {},
      env: {
        ...env,
        XDG_CONFIG_HOME: configHome,
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })

    expect(config.authConfig['//registry.npmjs.org/:_authToken']).toBe('workspace-token')
  } finally {
    if (originalXdg != null) {
      process.env.XDG_CONFIG_HOME = originalXdg
    } else {
      delete process.env.XDG_CONFIG_HOME
    }
  }
})

describe('unresolved ${VAR} placeholders in .npmrc auth values', () => {
  // Regression suite for https://github.com/pnpm/pnpm/issues/11513: actions/setup-node
  // writes `_authToken=${NODE_AUTH_TOKEN}` to .npmrc, and when the user relies on OIDC
  // trusted publishing without setting NODE_AUTH_TOKEN, the literal placeholder must not
  // surface as a bearer token — otherwise the registry sees `Bearer ${NODE_AUTH_TOKEN}`
  // and rejects the publish.
  let originalXdg: string | undefined
  let configHome: string
  let userconfig: string

  beforeEach(() => {
    prepareEmpty()
    fs.writeFileSync('.npmrc', '', 'utf8')
    fs.mkdirSync('user-home')
    userconfig = path.resolve('user-home', '.npmrc')
    fs.writeFileSync(userconfig, '//registry.npmjs.org/:_authToken=${NODE_AUTH_TOKEN}\n', 'utf8')
    // Isolate from the developer's real ~/.config/pnpm/auth.ini, which on a maintainer's
    // machine often contains a working npm token that would otherwise satisfy the assertion.
    configHome = path.resolve('xdg-config')
    fs.mkdirSync(path.join(configHome, 'pnpm'), { recursive: true })
    originalXdg = process.env.XDG_CONFIG_HOME
    process.env.XDG_CONFIG_HOME = configHome
  })

  afterEach(() => {
    if (originalXdg != null) {
      process.env.XDG_CONFIG_HOME = originalXdg
    } else {
      delete process.env.XDG_CONFIG_HOME
    }
  })

  test('drops the placeholder when the env var is unset', async () => {
    const { config } = await getConfig({
      cliOptions: {
        userconfig,
      },
      env: { ...env, XDG_CONFIG_HOME: configHome }, // NODE_AUTH_TOKEN intentionally unset
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.authConfig['//registry.npmjs.org/:_authToken']).toBe('')
  })

  test('substitutes normally when the env var is set', async () => {
    const { config } = await getConfig({
      cliOptions: {
        userconfig,
      },
      env: { ...env, XDG_CONFIG_HOME: configHome, NODE_AUTH_TOKEN: 'real-token' },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.authConfig['//registry.npmjs.org/:_authToken']).toBe('real-token')
  })

  test('only drops the unresolved placeholder, preserving resolved ones and defaults', async () => {
    // Same value contains one resolvable placeholder, one unresolved bare placeholder,
    // and one placeholder with a `-default` fallback. The unresolved one becomes ''
    // but the other two must still expand. Guards against the original implementation
    // that stripped every `${...}` on any substitution failure.
    fs.writeFileSync(
      userconfig,
      '//registry.test/:_authToken=${SET}-${UNSET}-${DEFAULTED-fallback}\n',
      'utf8'
    )

    const { config } = await getConfig({
      cliOptions: {
        userconfig,
      },
      env: { ...env, XDG_CONFIG_HOME: configHome, SET: 'AAA' }, // UNSET, DEFAULTED unset
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.authConfig['//registry.test/:_authToken']).toBe('AAA--fallback')
  })

  test('explicit `undefined` value in env is treated as unset for `${VAR-default}` fallbacks', async () => {
    // Callers that construct the env object directly (notably tests) commonly use
    // `{ KEY: undefined }` to model an unset variable. `${VAR-default}` must then
    // resolve to `default`, matching the `Record<string, string | undefined>` contract.
    fs.writeFileSync(
      userconfig,
      '//registry.test/:_authToken=${EXPLICIT_UNDEF-fallback}\n',
      'utf8'
    )

    const { config } = await getConfig({
      cliOptions: {
        userconfig,
      },
      env: { ...env, XDG_CONFIG_HOME: configHome, EXPLICIT_UNDEF: undefined },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.authConfig['//registry.test/:_authToken']).toBe('fallback')
  })
})

describe('unscoped credentials are pinned to the registry declared in their source file', () => {
  // Each .npmrc / auth.ini gets its unscoped credential keys rewritten to
  // URL-scoped form using the same source's `registry=` value (or the npmjs
  // default if it has none). A later layer overriding `registry=` therefore
  // cannot rebind the credential to its own registry — the credential is
  // already pinned to the URL its author intended.
  let originalXdg: string | undefined
  let configHome: string
  let userconfig: string

  beforeEach(() => {
    prepareEmpty()
    fs.mkdirSync('user-home')
    userconfig = path.resolve('user-home', '.npmrc')
    configHome = path.resolve('xdg-config')
    fs.mkdirSync(path.join(configHome, 'pnpm'), { recursive: true })
    originalXdg = process.env.XDG_CONFIG_HOME
    process.env.XDG_CONFIG_HOME = configHome
  })

  afterEach(() => {
    if (originalXdg != null) {
      process.env.XDG_CONFIG_HOME = originalXdg
    } else {
      delete process.env.XDG_CONFIG_HOME
    }
  })

  test('pins user-level _authToken to that file\'s registry, never the workspace registry', async () => {
    fs.writeFileSync(userconfig, 'registry=https://trusted.example.com/\n_authToken=user-secret\n', 'utf8')
    fs.writeFileSync('.npmrc', 'registry=https://attacker.example.com/\n', 'utf8')

    const { config } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.configByUri).toMatchObject({
      '//trusted.example.com/': { '@': { authToken: 'user-secret' } },
    })
    expect(config.configByUri['//attacker.example.com/']).toBeUndefined()
  })

  test('pins user-level _auth (basic) the same way', async () => {
    // cspell:disable-next-line
    fs.writeFileSync(userconfig, 'registry=https://trusted.example.com/\n_auth=dXNlcjpwYXNz\n', 'utf8')
    fs.writeFileSync('.npmrc', 'registry=https://attacker.example.com/\n', 'utf8')

    const { config } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.configByUri).toMatchObject({
      '//trusted.example.com/': { '@': { basicAuth: { username: 'user', password: 'pass' } } },
    })
    expect(config.configByUri['//attacker.example.com/']).toBeUndefined()
  })

  test('pins user-level username/_password the same way', async () => {
    // cspell:disable-next-line
    fs.writeFileSync(userconfig, 'registry=https://trusted.example.com/\nusername=alice\n_password=cGFzcw==\n', 'utf8')
    fs.writeFileSync('.npmrc', 'registry=https://attacker.example.com/\n', 'utf8')

    const { config } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.configByUri).toMatchObject({
      '//trusted.example.com/': { '@': { basicAuth: { username: 'alice', password: 'pass' } } },
    })
    expect(config.configByUri['//attacker.example.com/']).toBeUndefined()
  })

  test('auth.ini with no registry of its own falls back to the npmjs default', async () => {
    // The split-file case: ~/.npmrc declares a registry but no creds; auth.ini
    // declares an unscoped credential with no registry. Each file rescopes in
    // isolation, so the credential pins to the builtin npmjs default — NOT to
    // whatever the workspace later overrides the merged registry to.
    fs.writeFileSync(userconfig, 'registry=https://trusted.example.com/\n', 'utf8')
    fs.writeFileSync(
      path.join(configHome, 'pnpm', 'auth.ini'),
      '_authToken=user-secret\n',
      'utf8'
    )
    fs.writeFileSync('.npmrc', 'registry=https://attacker.example.com/\n', 'utf8')

    const { config } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.configByUri).toMatchObject({
      '//registry.npmjs.org/': { '@': { authToken: 'user-secret' } },
    })
    expect(config.configByUri['//attacker.example.com/']).toBeUndefined()
    expect(config.configByUri['//trusted.example.com/']).toBeUndefined()
  })

  test('user-level credentials work when no workspace .npmrc exists', async () => {
    fs.writeFileSync(userconfig, 'registry=https://trusted.example.com/\n_authToken=user-secret\n', 'utf8')

    const { config } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.configByUri).toMatchObject({
      '//trusted.example.com/': { '@': { authToken: 'user-secret' } },
    })
  })

  test('workspace-supplied unscoped credentials pin to the workspace registry', async () => {
    fs.writeFileSync(userconfig, '', 'utf8')
    fs.writeFileSync('.npmrc', 'registry=https://workspace.example.com/\n_authToken=workspace-token\n', 'utf8')

    const { config } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.configByUri).toMatchObject({
      '//workspace.example.com/': { '@': { authToken: 'workspace-token' } },
    })
  })

  test('explicit URL-scoped credentials pass through unchanged', async () => {
    fs.writeFileSync(
      userconfig,
      'registry=https://trusted.example.com/\n//trusted.example.com/:_authToken=user-secret\n',
      'utf8'
    )
    fs.writeFileSync('.npmrc', 'registry=https://attacker.example.com/\n', 'utf8')

    const { config, warnings } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(config.configByUri).toMatchObject({
      '//trusted.example.com/': { '@': { authToken: 'user-secret' } },
    })
    // URL-scoped tokens should NOT trigger the deprecation warning.
    expect(warnings.join('\n')).not.toMatch(/deprecated/i)
  })

  test('CLI --registry override does not pull an unscoped user-level token along', async () => {
    // Same trust boundary as the workspace case: an unscoped token is ambient
    // and shouldn't follow whatever registry the CLI happens to point at.
    fs.writeFileSync(userconfig, '_authToken=user-secret\n', 'utf8')

    const { config } = await getConfig({
      cliOptions: { userconfig, registry: 'https://attacker.example.com/' },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    // The token rescoped to the npmjs default when the user file was read.
    expect(config.configByUri).toMatchObject({
      '//registry.npmjs.org/': { '@': { authToken: 'user-secret' } },
    })
    expect(config.configByUri['//attacker.example.com/']).toBeUndefined()
  })

  test('pins inline client cert/key to the file\'s registry, never the workspace registry', async () => {
    const inlineCert = '-----BEGIN CERTIFICATE-----\\ncertbody\\n-----END CERTIFICATE-----'
    const inlineKey = '-----BEGIN PRIVATE KEY-----\\nkeybody\\n-----END PRIVATE KEY-----'
    fs.writeFileSync(
      userconfig,
      `registry=https://trusted.example.com/\ncert=${inlineCert}\nkey=${inlineKey}\n`,
      'utf8'
    )
    fs.writeFileSync('.npmrc', 'registry=https://attacker.example.com/\n', 'utf8')

    const { config } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    // `\n` escapes are expanded to real newlines by getNetworkConfigs.
    expect(config.configByUri['//trusted.example.com/']?.tls).toMatchObject({
      cert: inlineCert.replace(/\\n/g, '\n'),
      key: inlineKey.replace(/\\n/g, '\n'),
    })
    expect(config.configByUri['//attacker.example.com/']).toBeUndefined()
  })

})

describe('unscoped credential deprecation warning', () => {
  // pnpm warns whenever it reads any unscoped auth value from an .npmrc /
  // auth.ini, regardless of whether the rebind defense fires. URL-scoped tokens
  // have been npm's recommended pattern since npm@9, and unscoped credentials
  // are slated for removal in a future major release.
  let originalXdg: string | undefined
  let configHome: string
  let userconfig: string

  beforeEach(() => {
    prepareEmpty()
    fs.mkdirSync('user-home')
    userconfig = path.resolve('user-home', '.npmrc')
    configHome = path.resolve('xdg-config')
    fs.mkdirSync(path.join(configHome, 'pnpm'), { recursive: true })
    originalXdg = process.env.XDG_CONFIG_HOME
    process.env.XDG_CONFIG_HOME = configHome
  })

  afterEach(() => {
    if (originalXdg != null) {
      process.env.XDG_CONFIG_HOME = originalXdg
    } else {
      delete process.env.XDG_CONFIG_HOME
    }
  })

  test('warns about unscoped _authToken in user .npmrc', async () => {
    fs.writeFileSync(userconfig, 'registry=https://example.com/\n_authToken=secret\n', 'utf8')

    const { warnings } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(warnings.find(w => w.includes('Unscoped per-registry settings'))).toBeDefined()
    expect(warnings.find(w => w.includes('_authToken'))).toBeDefined()
    expect(warnings.find(w => w.includes(userconfig))).toBeDefined()
  })

  test('warns about unscoped _auth, username, _password', async () => {
    // _auth and _password are base64-encoded per npm convention.
    // cspell:disable-next-line
    fs.writeFileSync(userconfig, '_auth=dXNlcjpwYXNz\nusername=alice\n_password=cGFzcw==\n', 'utf8')

    const { warnings } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    const warning = warnings.find(w => w.includes('Unscoped per-registry settings'))
    expect(warning).toBeDefined()
    expect(warning).toContain('_auth')
    expect(warning).toContain('username')
    expect(warning).toContain('_password')
  })

  test('warns about unscoped credentials in workspace .npmrc too', async () => {
    fs.writeFileSync('.npmrc', 'registry=https://workspace.example.com/\n_authToken=workspace-token\n', 'utf8')

    const { warnings } = await getConfig({
      cliOptions: {},
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    const warning = warnings.find(w => w.includes('Unscoped per-registry settings'))
    expect(warning).toBeDefined()
    expect(warning).toContain(path.resolve('.npmrc'))
  })

  test('does not warn when only URL-scoped credentials are present', async () => {
    fs.writeFileSync(
      userconfig,
      'registry=https://example.com/\n//example.com/:_authToken=secret\n',
      'utf8'
    )

    const { warnings } = await getConfig({
      cliOptions: { userconfig },
      env: { ...env, XDG_CONFIG_HOME: configHome },
      packageManager: { name: 'pnpm', version: '1.0.0' },
      workspaceDir: process.cwd(),
    })

    expect(warnings.find(w => w.includes('Unscoped per-registry settings'))).toBeUndefined()
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

// hoist → hoistPattern processing is done in @pnpm/cli.utils
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

// public-hoist-pattern normalization is done in @pnpm/cli.utils
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

  writeYamlFileSync('pnpm-workspace.yaml', {
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
  expect(config.authConfig['foo']).toBe('bar')
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

test('workspacePackagePatterns defaults to ["."] when pnpm-workspace.yaml has no packages field', async () => {
  const workspaceDir = path.join(import.meta.dirname, 'fixtures/workspace-yaml-without-packages')
  process.chdir(workspaceDir)
  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir,
  })

  expect(config.workspacePackagePatterns).toEqual(['.'])
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

    writeYamlFileSync('pnpm-workspace.yaml', {
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

test('non-auth settings in npmrc do not produce warnings', async () => {
  prepare()

  const npmrc = [
    'typo-setting=true',
    ' ',
    'mistake-setting=false',
    '//foo.bar:_authToken=aaa',
    '@qar:registry=https://registry.example.org/',
  ].join('\n')
  fs.writeFileSync('.npmrc', npmrc, 'utf8')

  // Non-auth settings like typo-setting and mistake-setting are no longer
  // read from .npmrc, so they won't trigger unknown setting warnings.
  const { warnings } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(warnings).toStrictEqual([])
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
  const { config } = await getConfig({
    cliOptions: {
      userconfig: path.resolve('user-home', '.npmrc'),
    },
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
  fs.writeFileSync('.npmrc', 'registry = https://project-local.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {
      userconfig: path.resolve('user-home', '.npmrc'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.registry).toBe('https://project-local.example.test')
  expect(config.userConfig).toEqual({ registry: 'https://registry.example.test' })
})

test('getConfig() reads userconfig from PNPM_CONFIG_USERCONFIG env var', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry = https://registry.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      PNPM_CONFIG_USERCONFIG: path.resolve('user-home', '.npmrc'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.userConfig).toEqual({ registry: 'https://registry.example.test' })
})

test('getConfig() reads userconfig from pnpm_config_userconfig env var', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry = https://registry.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      pnpm_config_userconfig: path.resolve('user-home', '.npmrc'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.userConfig).toEqual({ registry: 'https://registry.example.test' })
})

test('getConfig() reads userconfig from PNPM_CONFIG_NPMRC_AUTH_FILE env var', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry = https://registry.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      PNPM_CONFIG_NPMRC_AUTH_FILE: path.resolve('user-home', '.npmrc'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.userConfig).toEqual({ registry: 'https://registry.example.test' })
})

test('getConfig() reads userconfig from pnpm_config_npmrc_auth_file env var', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry = https://registry.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      pnpm_config_npmrc_auth_file: path.resolve('user-home', '.npmrc'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.userConfig).toEqual({ registry: 'https://registry.example.test' })
})

// Locks in the precedence so future refactors don't accidentally flip it.
test('getConfig() prefers pnpm_config_userconfig over PNPM_CONFIG_USERCONFIG when both are set', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', 'upper.npmrc'), 'registry = https://upper.example.test', 'utf-8')
  fs.writeFileSync(path.resolve('user-home', 'lower.npmrc'), 'registry = https://lower.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      PNPM_CONFIG_USERCONFIG: path.resolve('user-home', 'upper.npmrc'),
      pnpm_config_userconfig: path.resolve('user-home', 'lower.npmrc'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.userConfig).toEqual({ registry: 'https://lower.example.test' })
})

// actions/setup-node writes auth to ${runner.temp}/.npmrc and sets NPM_CONFIG_USERCONFIG;
// pnpm honors it as a low-priority compatibility fallback for that flow.
test('getConfig() reads userconfig from NPM_CONFIG_USERCONFIG env var', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry = https://registry.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      NPM_CONFIG_USERCONFIG: path.resolve('user-home', '.npmrc'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.userConfig).toEqual({ registry: 'https://registry.example.test' })
})

test('getConfig() reads userconfig from npm_config_userconfig env var', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry = https://registry.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      npm_config_userconfig: path.resolve('user-home', '.npmrc'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.userConfig).toEqual({ registry: 'https://registry.example.test' })
})

test('getConfig() prefers PNPM_CONFIG_USERCONFIG over NPM_CONFIG_USERCONFIG when both are set', async () => {
  prepareEmpty()
  fs.mkdirSync('user-home')
  fs.writeFileSync(path.resolve('user-home', 'pnpm.npmrc'), 'registry = https://pnpm.example.test', 'utf-8')
  fs.writeFileSync(path.resolve('user-home', 'npm.npmrc'), 'registry = https://npm.example.test', 'utf-8')
  const { config } = await getConfig({
    cliOptions: {},
    env: {
      ...env,
      PNPM_CONFIG_USERCONFIG: path.resolve('user-home', 'pnpm.npmrc'),
      NPM_CONFIG_USERCONFIG: path.resolve('user-home', 'npm.npmrc'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.userConfig).toEqual({ registry: 'https://pnpm.example.test' })
})

// An empty NPM_CONFIG_USERCONFIG (e.g. `export NPM_CONFIG_USERCONFIG=`) must be
// treated as unset. Otherwise it short-circuits the fallback chain and resolves
// to the cwd, returning an empty/invalid auth config instead of ~/.npmrc.
test('getConfig() ignores an empty NPM_CONFIG_USERCONFIG and falls back to ~/.npmrc', async () => {
  prepareEmpty()
  const homedirSpy = jest.spyOn(os, 'homedir').mockReturnValue(path.resolve('user-home'))
  try {
    fs.mkdirSync('user-home')
    fs.writeFileSync(path.resolve('user-home', '.npmrc'), 'registry = https://home.example.test', 'utf-8')
    const { config } = await getConfig({
      cliOptions: {},
      env: {
        ...env,
        NPM_CONFIG_USERCONFIG: '',
        npm_config_userconfig: '',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
    expect(config.userConfig).toEqual({ registry: 'https://home.example.test' })
  } finally {
    homedirSpy.mockRestore()
  }
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

// Regression for https://github.com/pnpm/pnpm/issues/11624.
test('getConfig() resolves a relative cafile= from .npmrc against the npmrc directory, not process.cwd()', async () => {
  prepareEmpty()
  const projectDir = path.resolve('project')
  fs.mkdirSync(path.join(projectDir, 'certs'), { recursive: true })
  fs.writeFileSync(
    path.join(projectDir, 'certs', 'ca.pem'),
    'relative-ca\n-----END CERTIFICATE-----'
  )
  fs.writeFileSync(path.join(projectDir, '.npmrc'), 'cafile=certs/ca.pem\n')

  // process.cwd() is the prepareEmpty() root, *not* projectDir — i.e. the same
  // shape as `pnpm --dir <projectDir> install` invoked from a sibling cwd.
  const { config } = await getConfig({
    cliOptions: { dir: projectDir },
    packageManager: { name: 'pnpm', version: '1.0.0' },
  })

  expect(config.ca).toStrictEqual(['relative-ca\n-----END CERTIFICATE-----'])
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
  expect(config.configByUri['//registry.example.com/']?.tls).toMatchObject({
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

    writeYamlFileSync('pnpm-workspace.yaml', {
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
    writeYamlFileSync('pnpm-workspace.yaml', {
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
  prepare()

  fs.writeFileSync('.npmrc', 'registry=${ENV_VAR_123}', 'utf8')
  const { warnings } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  const expected = [
    expect.stringContaining('Ignored project-level request destination "registry"'),
  ]

  expect(warnings).toEqual(expect.arrayContaining(expected))
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

test('return a warning if a package.json has a legacy "pnpm" field with ignored settings', async () => {
  const prefix = f.find('pkg-with-legacy-pnpm-field')
  const { warnings } = await getConfig({
    cliOptions: { dir: prefix },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(warnings).toStrictEqual([
    'The "pnpm" field in package.json is no longer read by pnpm. The following keys were ignored: "pnpm.overrides", "pnpm.patchedDependencies". See https://pnpm.io/settings for the new home of each setting.',
  ])
})

test('do not return a warning if a package.json "pnpm" field only contains keys that are still actively read (e.g. "pnpm.app")', async () => {
  const prefix = f.find('pkg-with-pnpm-app-field')
  const { warnings } = await getConfig({
    cliOptions: { dir: prefix },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })

  expect(warnings).toStrictEqual([])
})

test('do not return a warning if a package.json "pnpm" field only contains keys unrelated to migrated settings (e.g. set by third-party tooling)', async () => {
  const prefix = f.find('pkg-with-unknown-pnpm-field')
  const { warnings } = await getConfig({
    cliOptions: { dir: prefix },
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

test('project .npmrc does not expand env variables into registry keys', async () => {
  const oldEnv = process.env
  process.env = {
    ...oldEnv,
    FOO: 'registry',
  }

  const { config, warnings } = await getConfig({
    cliOptions: {
      dir: f.find('has-env-in-key'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.registry).toBe('https://registry.npmjs.org/')
  expect(warnings).toEqual(expect.arrayContaining([
    expect.stringContaining('Ignored project-level request destination "${FOO}"'),
  ]))

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

// shamefullyHoist → publicHoistPattern conversion is done in @pnpm/cli.utils
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

  writeYamlFileSync('pnpm-workspace.yaml', {
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
  let PNPM_TEST_HOST: string | undefined

  beforeEach(() => {
    XDG_CONFIG_HOME = process.env.XDG_CONFIG_HOME
    PNPM_TEST_HOST = process.env.PNPM_TEST_HOST
  })

  afterEach(() => {
    if (XDG_CONFIG_HOME == null) {
      delete process.env.XDG_CONFIG_HOME
    } else {
      process.env.XDG_CONFIG_HOME = XDG_CONFIG_HOME
    }
    if (PNPM_TEST_HOST == null) {
      delete process.env.PNPM_TEST_HOST
    } else {
      process.env.PNPM_TEST_HOST = PNPM_TEST_HOST
    }
  })

  test('reads config from global config.yaml', async () => {
    prepareEmpty()

    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFileSync('.config/pnpm/config.yaml', {
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
    expect(config.dangerouslyAllowAllBuilds).toBeDefined()
  })

  test('expands request destination values from trusted global config.yaml', async () => {
    prepareEmpty()

    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFileSync('.config/pnpm/config.yaml', {
      pnprServer: 'https://${PNPM_TEST_HOST}/pnpr/',
      registry: 'https://${PNPM_TEST_HOST}/npm/',
    })

    process.env.XDG_CONFIG_HOME = path.resolve('.config')
    process.env.PNPM_TEST_HOST = 'trusted.example.com'

    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.pnprServer).toBe('https://trusted.example.com/pnpr/')
    expect(config.registry).toBe('https://trusted.example.com/npm/')
    expect(config.registries.default).toBe('https://trusted.example.com/npm/')
  })

  test('reads user-level preference settings from global config.yaml', async () => {
    prepareEmpty()

    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFileSync('.config/pnpm/config.yaml', {
      scriptShell: '/usr/local/bin/bash',
      shellEmulator: true,
      updateNotifier: false,
      stateDir: '/custom/state',
      trustPolicy: 'no-downgrade',
      trustPolicyExclude: ['legacy-pkg'],
      registrySupportsTimeField: true,
      sideEffectsCache: false,
      strictDepBuilds: true,
      useStderr: true,
      verifyDepsBeforeRun: 'error',
      verifyStoreIntegrity: false,
      frozenStore: true,
      virtualStoreDir: '/custom/.pnpm',
      virtualStoreDirMaxLength: 80,
    })

    process.env.XDG_CONFIG_HOME = path.resolve('.config')

    const { config, warnings } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.scriptShell).toBe('/usr/local/bin/bash')
    expect(config.shellEmulator).toBe(true)
    expect(config.updateNotifier).toBe(false)
    expect(config.stateDir).toBe('/custom/state')
    expect(config.trustPolicy).toBe('no-downgrade')
    expect(config.trustPolicyExclude).toEqual(['legacy-pkg'])
    expect(config.registrySupportsTimeField).toBe(true)
    expect(config.sideEffectsCache).toBe(false)
    expect(config.strictDepBuilds).toBe(true)
    expect(config.useStderr).toBe(true)
    expect(config.verifyDepsBeforeRun).toBe('error')
    expect(config.verifyStoreIntegrity).toBe(false)
    expect(config.frozenStore).toBe(true)
    expect(config.virtualStoreDir).toBe('/custom/.pnpm')
    expect(config.virtualStoreDirMaxLength).toBe(80)
    expect(warnings.find((w) => w.includes('global config file'))).toBeUndefined()
  })

  test('warns when global config.yaml contains settings that are not allowed in the global config', async () => {
    prepareEmpty()

    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFileSync('.config/pnpm/config.yaml', {
      dangerouslyAllowAllBuilds: true,
      nodeLinker: 'hoisted',
      hoistPattern: ['*eslint*'],
    })

    process.env.XDG_CONFIG_HOME = path.resolve('.config')

    const { config, warnings } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    // Allowed setting is still applied.
    expect(config.dangerouslyAllowAllBuilds).toBe(true)
    // Ignored settings do not leak into the config.
    expect(config.nodeLinker).not.toBe('hoisted')
    expect(config.hoistPattern).toEqual(['*'])

    const warning = warnings.find((w) => w.includes('global config file'))
    expect(warning).toBeDefined()
    expect(warning).toContain('"nodeLinker"')
    expect(warning).toContain('"hoistPattern"')
    expect(warning).not.toContain('"dangerouslyAllowAllBuilds"')
    expect(warning).toContain(path.join(process.env.XDG_CONFIG_HOME!, 'pnpm', 'config.yaml'))
    expect(warning).toContain('pnpm-workspace.yaml')
    expect(warning).toContain('https://pnpm.io/11.x/config-dependencies')
    expect(warning).not.toContain('.npmrc')
  })

  test('reads proxy settings from global config.yaml', async () => {
    prepareEmpty()

    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFileSync('.config/pnpm/config.yaml', {
      httpProxy: 'http://proxy.example.com:8080',
      httpsProxy: 'http://proxy.example.com:8443',
      noProxy: 'localhost,127.0.0.1',
    })

    process.env.XDG_CONFIG_HOME = path.resolve('.config')

    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.httpProxy).toBe('http://proxy.example.com:8080')
    expect(config.httpsProxy).toBe('http://proxy.example.com:8443')
    expect(config.noProxy).toBe('localhost,127.0.0.1')
  })

  test('proxy settings from global config.yaml override .npmrc', async () => {
    prepareEmpty()

    // Set proxy in .npmrc (npm-style keys)
    fs.writeFileSync('.npmrc', 'https-proxy=http://npmrc-proxy.example.com:8080', 'utf8')

    // Set different proxy in global config.yaml
    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFileSync('.config/pnpm/config.yaml', {
      httpsProxy: 'http://yaml-proxy.example.com:9090',
    })

    process.env.XDG_CONFIG_HOME = path.resolve('.config')

    const { config } = await getConfig({
      cliOptions: {},
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    // Global YAML should override .npmrc
    expect(config.httpsProxy).toBe('http://yaml-proxy.example.com:9090')
  })

  test('CLI flags override proxy settings from global config.yaml', async () => {
    prepareEmpty()

    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFileSync('.config/pnpm/config.yaml', {
      httpsProxy: 'http://yaml-proxy.example.com:9090',
    })

    process.env.XDG_CONFIG_HOME = path.resolve('.config')

    const { config } = await getConfig({
      cliOptions: {
        'https-proxy': 'http://cli-proxy.example.com:7070',
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.httpsProxy).toBe('http://cli-proxy.example.com:7070')
  })

  // npmrcAuthFile in global config.yaml is a deliberate pnpm-native setting and should
  // not be silently overridden by an ambient NPM_CONFIG_USERCONFIG (e.g. from a CI runner).
  test('npmrcAuthFile from global config.yaml takes precedence over NPM_CONFIG_USERCONFIG', async () => {
    prepareEmpty()
    fs.mkdirSync('user-home')
    fs.writeFileSync(path.resolve('user-home', 'yaml.npmrc'), 'registry = https://yaml.example.test', 'utf-8')
    fs.writeFileSync(path.resolve('user-home', 'npm.npmrc'), 'registry = https://npm.example.test', 'utf-8')

    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFileSync('.config/pnpm/config.yaml', {
      npmrcAuthFile: path.resolve('user-home', 'yaml.npmrc'),
    })

    process.env.XDG_CONFIG_HOME = path.resolve('.config')

    const { config } = await getConfig({
      cliOptions: {},
      env: {
        ...env,
        NPM_CONFIG_USERCONFIG: path.resolve('user-home', 'npm.npmrc'),
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
      workspaceDir: process.cwd(),
    })

    expect(config.userConfig).toEqual({ registry: 'https://yaml.example.test' })
  })
})

test('proxy settings are still read from .npmrc', async () => {
  prepareEmpty()

  fs.writeFileSync('.npmrc', 'https-proxy=http://npmrc-proxy.example.com:8080\nproxy=http://npmrc-http-proxy.example.com:3128\nno-proxy=internal.example.com', 'utf8')

  const { config } = await getConfig({
    cliOptions: {},
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })

  expect(config.httpsProxy).toBe('http://npmrc-proxy.example.com:8080')
  expect(config.httpProxy).toBe('http://npmrc-proxy.example.com:8080')
  expect(config.noProxy).toBe('internal.example.com')
})

test('lockfile: false in pnpm-workspace.yaml sets useLockfile to false', async () => {
  prepareEmpty()

  writeYamlFileSync('pnpm-workspace.yaml', {
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

  writeYamlFileSync('pnpm-workspace.yaml', {
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

test('ci disables enableGlobalVirtualStore by default', async () => {
  prepareEmpty()

  writeYamlFileSync('pnpm-workspace.yaml', {
    ci: true,
  })

  const { config } = await getConfig({
    cliOptions: {},
    env,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })

  expect(config.enableGlobalVirtualStore).toBe(false)
})

test('ci respects explicit enableGlobalVirtualStore from config', async () => {
  prepareEmpty()

  writeYamlFileSync('pnpm-workspace.yaml', {
    ci: true,
    enableGlobalVirtualStore: true,
  })

  const { config } = await getConfig({
    cliOptions: {},
    env,
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
    workspaceDir: process.cwd(),
  })

  expect(config.enableGlobalVirtualStore).toBe(true)
})

test('pnpm_config_git_branch_lockfile env var overrides git-branch-lockfile from pnpm-workspace.yaml in useGitBranchLockfile', async () => {
  prepareEmpty()

  writeYamlFileSync('pnpm-workspace.yaml', {
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

test('GVS: workspace manifest allowBuilds takes precedence over global config.yaml dangerouslyAllowAllBuilds', async () => {
  prepareEmpty()

  const prevXdgConfigHome = process.env.XDG_CONFIG_HOME

  const globalDir = path.join(import.meta.dirname, 'global', GLOBAL_LAYOUT_VERSION)

  try {
    process.env.XDG_CONFIG_HOME = path.resolve('.config')

    fs.mkdirSync(path.join(process.env.XDG_CONFIG_HOME, 'pnpm'), { recursive: true })
    writeYamlFileSync(path.join(process.env.XDG_CONFIG_HOME, 'pnpm', 'config.yaml'), {
      dangerouslyAllowAllBuilds: true,
    })

    fs.mkdirSync(globalDir, { recursive: true })
    writeYamlFileSync(path.join(globalDir, 'pnpm-workspace.yaml'), {
      allowBuilds: { '@some/pkg': true, esbuild: true },
    })

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

    expect(config.enableGlobalVirtualStore).toBe(true)
    expect(config.allowBuilds).toStrictEqual({ '@some/pkg': true, esbuild: true })
    // The dangerouslyAllowAllBuilds value from the already-loaded global config.yaml
    // is preserved when workspace manifest settings are applied after
    // extractAndRemoveDependencyBuildOptions strips the workspace build options.
    expect(config.dangerouslyAllowAllBuilds).toBe(true)
  } finally {
    if (prevXdgConfigHome === undefined) {
      delete process.env.XDG_CONFIG_HOME
    } else {
      process.env.XDG_CONFIG_HOME = prevXdgConfigHome
    }
    fs.rmSync(globalDir, { recursive: true, force: true })
    const parentGlobalDir = path.join(import.meta.dirname, 'global')
    if (fs.existsSync(parentGlobalDir)) {
      fs.rmSync(parentGlobalDir, { recursive: true, force: true })
    }
  }
})

test('GVS: global config.yaml dangerouslyAllowAllBuilds is preserved when no workspace manifest exists', async () => {
  prepareEmpty()

  const prevXdgConfigHome = process.env.XDG_CONFIG_HOME

  try {
    // Set up global config.yaml with a build policy
    fs.mkdirSync('.config/pnpm', { recursive: true })
    writeYamlFileSync('.config/pnpm/config.yaml', {
      dangerouslyAllowAllBuilds: true,
    })
    process.env.XDG_CONFIG_HOME = path.resolve('.config')

    // No global pnpm-workspace.yaml
    // intentionally do not write a workspace manifest

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

    // For global installs, enableGlobalVirtualStore defaults to true.
    expect(config.enableGlobalVirtualStore).toBe(true)
    // The key assertion: global config.yaml policy should NOT be wiped by the GVS
    // allowBuilds = {} default. Previously this block set allowBuilds
    // before globalDepsBuildConfig was re-applied, so hasDependencyBuildOptions
    // saw allowBuilds = {} and skipped re-application, silently losing
    // dangerouslyAllowAllBuilds.
    expect(config.dangerouslyAllowAllBuilds).toBe(true)
    // allowBuilds should remain null — dangerouslyAllowAllBuilds IS the policy
    expect(config.allowBuilds).toBeUndefined()
  } finally {
    if (prevXdgConfigHome === undefined) {
      delete process.env.XDG_CONFIG_HOME
    } else {
      process.env.XDG_CONFIG_HOME = prevXdgConfigHome
    }
  }
})
