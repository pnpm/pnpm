import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
import { packageManager } from '@pnpm/cli.meta'
import {
  getPackageManagerBootstrapConfig,
  PIN_DECISION_SETTINGS,
} from '@pnpm/config.reader'
import { createCliConfigArgs } from '@pnpm/exec.commands'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { tempDir } from '@pnpm/prepare'

interface MatrixCase {
  id: string
  pinSource: string
  pinVersion: string
  settingLocation: string
  readability: string
  settings?: {
    pmOnFail?: string
    lockfileDir?: string
    lockfile?: boolean
    globalDir?: string
  }
  entryPoint: string
  expected: {
    outcome: 'switch' | 'record' | 'nothing' | 'error'
    version?: string
    at?: string
  }
}

interface Fixture {
  PIN_DECISION_SETTINGS: string[]
  cases: MatrixCase[]
}

type ActualOutcome =
  | { outcome: 'switch', version: string }
  | { outcome: 'record', at: string }
  | { outcome: 'nothing' }
  | { outcome: 'error' }

let recordedAt: string | undefined
let switchedVersion: string | undefined
let switchedArgs: string[] | undefined

const closeStore = jest.fn<() => Promise<void>>(async () => {})
const createStoreController = jest.fn<(opts: object) => Promise<{
  ctrl: { close: typeof closeStore }
  dir: string
}>>(async () => ({
  ctrl: { close: closeStore },
  dir: '/store',
}))

const isPackageManagerResolved = jest.fn<() => boolean>(() => false)
const resolvePackageManagerIntegrities = jest.fn<(version: string, opts: { rootDir: string, save?: boolean }) => Promise<EnvLockfile>>(
  async (version, opts) => {
    if (opts.save !== false) {
      recordedAt = opts.rootDir
    }
    return {
      lockfileVersion: '9.0',
      importers: {
        '.': {
          configDependencies: {},
          packageManagerDependencies: {
            pnpm: { specifier: version, version },
            '@pnpm/exe': { specifier: version, version },
          },
        },
      },
      packages: {
        [`pnpm@${version}`]: { resolution: { integrity: 'sha512-pnpm' } },
        [`@pnpm/exe@${version}`]: { resolution: { integrity: 'sha512-exe' } },
      },
      snapshots: {
        [`pnpm@${version}`]: {},
        [`@pnpm/exe@${version}`]: {},
      },
    }
  }
)

const installPnpmToStore = jest.fn<(version: string, opts: object) => Promise<{ binDir: string }>>(
  async (version) => {
    switchedVersion = version
    return { binDir: '/store/bin' }
  }
)
const spawnPnpm = jest.fn<(pnpmBinPath: string, args: string[]) => Promise<{ status: number | null, signal: NodeJS.Signals | null }>>(
  async (_pnpmBinPath, args) => {
    switchedArgs = args
    return { status: 0, signal: null }
  }
)

const actualEnginePmCommands = await import('@pnpm/engine.pm.commands')
const maturePnpmVersionForRange = jest.fn<() => Promise<string>>(async () => packageManager.version)

jest.unstable_mockModule('./exit.js', () => ({
  exit: async () => {},
}))

jest.unstable_mockModule('@pnpm/installing.env-installer', () => ({
  isPackageManagerResolved,
  resolvePackageManagerIntegrities,
  readConfigDependencyLockfile: async () => undefined,
  resolveAndInstallConfigDeps: async () => {},
}))

jest.unstable_mockModule('@pnpm/store.connection-manager', () => ({
  createStoreController,
}))

jest.unstable_mockModule('@pnpm/engine.pm.commands', () => ({
  ...actualEnginePmCommands,
  installPnpmToStore,
  maturePnpmVersionForRange,
  spawnPnpm,
}))

const { updateManifestPinAndDecide } = actualEnginePmCommands
const { getConfig } = await import('./getConfig.js')
const { handlePackageManagerAndRuntimes, switchPastUnreadableConfig } = await import('./packageManagerCheck.js')
const { parseCliArgs } = await import('./parseCliArgs.js')

const fixturePath = path.resolve(import.meta.dirname, '../../../fixtures/pin-decision.json')
const fixture: Fixture = JSON.parse(fs.readFileSync(fixturePath, 'utf8'))

beforeEach(() => {
  recordedAt = undefined
  switchedVersion = undefined
  switchedArgs = undefined
  closeStore.mockClear()
  createStoreController.mockClear()
  installPnpmToStore.mockClear()
  isPackageManagerResolved.mockClear()
  isPackageManagerResolved.mockReturnValue(false)
  resolvePackageManagerIntegrities.mockClear()
  spawnPnpm.mockClear()
})

test('PIN_DECISION_SETTINGS matches shared fixture', () => {
  const actual = [...PIN_DECISION_SETTINGS].sort()
  const expected = [...fixture.PIN_DECISION_SETTINGS].sort()
  expect(actual).toEqual(expected)
})

test('pin_decision_matrix', async () => {
  expect(fixture.cases.length).toBeGreaterThan(0)
  for (const testCase of fixture.cases) {
    recordedAt = undefined
    switchedVersion = undefined
    // eslint-disable-next-line no-await-in-loop -- each case sets process.env.XDG_CONFIG_HOME
    const actual = await runMatrixCase(testCase)
    expect({ id: testCase.id, ...actual }).toEqual({ id: testCase.id, ...testCase.expected })
  }
})

async function runMatrixCase (testCase: MatrixCase): Promise<ActualOutcome> {
  const base = fs.realpathSync(tempDir(false))
  const workspaceRoot = path.join(base, 'workspace')
  const xdgConfigHome = path.join(base, 'xdg-config')
  const globalConfigDir = path.join(xdgConfigHome, 'pnpm')
  fs.mkdirSync(workspaceRoot, { recursive: true })
  fs.mkdirSync(globalConfigDir, { recursive: true })

  const version = testCase.pinVersion === 'running' ? packageManager.version : testCase.pinVersion
  const runDir = setupPinSource(workspaceRoot, testCase.pinSource, version)
  const cliFlags = setupSettings(
    workspaceRoot,
    globalConfigDir,
    testCase.pinSource,
    testCase.settingLocation,
    testCase.readability,
    testCase.settings ?? {}
  )

  const prevXdgConfigHome = process.env.XDG_CONFIG_HOME
  process.env.XDG_CONFIG_HOME = xdgConfigHome
  try {
    return await executeEntryPoint(workspaceRoot, runDir, testCase.entryPoint, cliFlags)
  } finally {
    if (prevXdgConfigHome === undefined) {
      delete process.env.XDG_CONFIG_HOME
    } else {
      process.env.XDG_CONFIG_HOME = prevXdgConfigHome
    }
  }
}

function setupPinSource (workspaceRoot: string, pinSource: string, version: string): string {
  switch (pinSource) {
    case 'packageManager': {
      fs.writeFileSync(
        path.join(workspaceRoot, 'package.json'),
        JSON.stringify({
          name: 'root',
          packageManager: `pnpm@${version}`,
        }),
        'utf8'
      )
      return workspaceRoot
    }
    case 'devEngines': {
      fs.writeFileSync(
        path.join(workspaceRoot, 'package.json'),
        JSON.stringify({
          name: 'root',
          devEngines: {
            packageManager: {
              name: 'pnpm',
              version,
            },
          },
        }),
        'utf8'
      )
      return workspaceRoot
    }
    case 'workspace-package-yaml': {
      fs.writeFileSync(
        path.join(workspaceRoot, 'package.yaml'),
        `name: root\ndevEngines:\n  packageManager:\n    name: pnpm\n    version: '${version}'\n`,
        'utf8'
      )
      return workspaceRoot
    }
    case 'nested-project': {
      fs.writeFileSync(
        path.join(workspaceRoot, 'package.json'),
        JSON.stringify({
          name: 'root',
          devEngines: {
            packageManager: {
              name: 'pnpm',
              version,
            },
          },
        }),
        'utf8'
      )
      const appDir = path.join(workspaceRoot, 'packages', 'app')
      fs.mkdirSync(appDir, { recursive: true })
      fs.writeFileSync(path.join(appDir, 'package.json'), JSON.stringify({ name: 'app' }), 'utf8')
      return fs.realpathSync(appDir)
    }
    default:
      throw new Error(`unknown pinSource: ${pinSource}`)
  }
}

function setupSettings (
  workspaceRoot: string,
  globalConfigDir: string,
  pinSource: string,
  settingLocation: string,
  readability: string,
  settings: NonNullable<MatrixCase['settings']>
): string[] {
  let workspaceYaml = pinSource === 'workspace-package-yaml' || pinSource === 'nested-project'
    ? 'packages:\n  - packages/*\n'
    : 'packages:\n  - .\n'
  const unreadable = readability === 'unreadable'
  const suffix = unreadable ? 'remoteSideEffectsCache: 123\n' : ''

  switch (settingLocation) {
    case 'global-config-yaml':
      fs.writeFileSync(path.join(globalConfigDir, 'config.yaml'), formatYamlSettings(settings) + suffix, 'utf8')
      break
    case 'pnpm-workspace-yaml':
      workspaceYaml += formatYamlSettings(settings) + suffix
      break
    case 'cli-config':
      workspaceYaml += suffix
      break
    case 'npmrc':
      writeTestNpmrc(workspaceRoot, settings, unreadable)
      break
    default:
      throw new Error(`unknown settingLocation: ${settingLocation}`)
  }

  fs.writeFileSync(path.join(workspaceRoot, 'pnpm-workspace.yaml'), workspaceYaml, 'utf8')
  return settingLocation === 'cli-config' ? buildCliFlags(settings) : []
}

function buildCliFlags (settings: NonNullable<MatrixCase['settings']>): string[] {
  const cliFlags: string[] = []
  if (settings.pmOnFail != null) cliFlags.push(`--config.pm-on-fail=${settings.pmOnFail}`)
  if (settings.lockfileDir != null) cliFlags.push(`--config.lockfile-dir=${settings.lockfileDir}`)
  if (settings.lockfile != null) cliFlags.push(`--config.lockfile=${String(settings.lockfile)}`)
  if (settings.globalDir != null) cliFlags.push(`--config.global-dir=${settings.globalDir}`)
  return cliFlags
}

function writeTestNpmrc (workspaceRoot: string, settings: NonNullable<MatrixCase['settings']>, unreadable: boolean): void {
  let npmrc = ''
  if (settings.pmOnFail != null) npmrc += `pm-on-fail=${settings.pmOnFail}\n`
  if (settings.lockfileDir != null) npmrc += `lockfile-dir=${settings.lockfileDir}\n`
  if (unreadable) npmrc += '//registry.example.com/:tokenHelper=/bin/echo s3cr3t\n'
  fs.writeFileSync(path.join(workspaceRoot, '.npmrc'), npmrc, 'utf8')
}

function formatYamlSettings (settings: NonNullable<MatrixCase['settings']>): string {
  let out = ''
  if (settings.pmOnFail != null) {
    out += `pmOnFail: ${settings.pmOnFail}\n`
  }
  if (settings.lockfileDir != null) {
    out += `lockfileDir: ${settings.lockfileDir}\n`
  }
  if (settings.lockfile != null) {
    out += `lockfile: ${String(settings.lockfile)}\n`
  }
  if (settings.globalDir != null) {
    out += `globalDir: ${settings.globalDir}\n`
  }
  return out
}

async function executeEntryPoint (
  workspaceRoot: string,
  runDir: string,
  entryPoint: string,
  cliFlags: string[]
): Promise<ActualOutcome> {
  const baseArgv = ['--dir', runDir, '--reporter=silent', ...cliFlags]

  switch (entryPoint) {
    case 'command':
      return runCliFlow(workspaceRoot, [...baseArgv, 'root'])
    case '--version':
      return runCliFlow(workspaceRoot, [...baseArgv, '--version'])
    case 'unknown-flag':
      return runCliFlow(workspaceRoot, [...baseArgv, 'root', '--definitely-unknown-flag'])
    case 'self-update':
      return runSelfUpdateFlow(workspaceRoot, [...baseArgv, 'self-update', '99.0.0'])
    default:
      throw new Error(`unknown entryPoint: ${entryPoint}`)
  }
}

async function runCliFlow (workspaceRoot: string, inputArgv: string[]): Promise<ActualOutcome> {
  try {
    return await runCliFlowThrowing(workspaceRoot, inputArgv)
  } catch {
    if (switchedVersion != null) {
      return { outcome: 'switch', version: switchedVersion }
    }
    return { outcome: 'error' }
  }
}

async function runCliFlowThrowing (workspaceRoot: string, inputArgv: string[]): Promise<ActualOutcome> {
  const parsedCliArgs = await parseCliArgs(inputArgv)
  const { cmd, options: cliOptions, unknownOptions, workspaceDir, rawCliConfig } = parsedCliArgs
  const rejectUnknownOptions = unknownOptions.size > 0
  for (const key of unknownOptions.keys()) {
    delete cliOptions[key]
  }
  const { config, context, allSettingsRead } = await loadTestConfig(cmd, cliOptions, workspaceDir, rawCliConfig)

  await handlePackageManagerAndRuntimes({
    cmd,
    cliOptions,
    config,
    context,
    rejectUnknownOptions,
    recordPinWithoutSwitch: allSettingsRead,
  })

  if (switchedVersion != null) return { outcome: 'switch', version: switchedVersion }
  if (rejectUnknownOptions) return { outcome: 'error' }
  if (recordedAt != null) return { outcome: 'record', at: relativeLockfileDir(workspaceRoot, recordedAt) }
  return { outcome: 'nothing' }
}

async function loadTestConfig (
  cmd: string | null,
  cliOptions: Record<string, unknown>,
  workspaceDir: string | undefined,
  rawCliConfig: Record<string, unknown> | undefined
) {
  const getConfigOpts = {
    excludeReporter: false,
    globalDirShouldAllowWrite: cmd !== 'root' && cmd !== 'prefix',
    workspaceDir,
    rawCliConfig,
    onlyInheritDlxSettingsFromLocal: false,
    forSelfUpdate: false,
    ignoreProjectNpmrc: false,
    printWarnings: false,
  }
  try {
    return { ...await getConfig(cliOptions, getConfigOpts), allSettingsRead: true }
  } catch (err: unknown) {
    const loadReadableConfig = async () => getConfig(cliOptions, {
      ...getConfigOpts,
      skipUnreadableSettings: true,
      printWarnings: false,
    })
    if (cmd == null && cliOptions.version === true) {
      return { ...await loadReadableConfig(), allSettingsRead: false }
    }
    await switchPastUnreadableConfig(loadReadableConfig, cliOptions)
    throw err
  }
}

async function runSelfUpdateFlow (workspaceRoot: string, inputArgv: string[]): Promise<ActualOutcome> {
  try {
    const parsedCliArgs = await parseCliArgs(inputArgv)
    const { options: cliOptions, workspaceDir, rawCliConfig } = parsedCliArgs
    const { config, context } = await getConfig(cliOptions, {
      excludeReporter: false,
      workspaceDir,
      rawCliConfig,
      forSelfUpdate: true,
      printWarnings: false,
    })
    const { decision } = await updateManifestPinAndDecide({ ...config, ...context }, '99.0.0')
    if (decision.action === 'record') {
      return { outcome: 'record', at: relativeLockfileDir(workspaceRoot, decision.at) }
    }
    if (decision.action === 'switch') {
      return { outcome: 'switch', version: decision.specifier }
    }
    return { outcome: 'nothing' }
  } catch {
    return { outcome: 'error' }
  }
}

function relativeLockfileDir (workspaceRoot: string, targetDir: string): string {
  const rel = path.relative(workspaceRoot, targetDir).replaceAll('\\', '/')
  return rel === '' ? '.' : rel
}

test('cli_config_reaches_every_child', async () => {
  const root = fs.realpathSync(tempDir(false))
  const xdgConfigHome = path.join(root, 'xdg-config')
  fs.mkdirSync(path.join(xdgConfigHome, 'pnpm'), { recursive: true })
  fs.writeFileSync(path.join(root, 'pnpm-workspace.yaml'), 'packages:\n  - .\n', 'utf8')
  fs.writeFileSync(
    path.join(root, 'package.json'),
    JSON.stringify({
      name: 'root',
      devEngines: {
        packageManager: {
          name: 'pnpm',
          version: packageManager.version,
          onFail: 'download',
        },
      },
    }),
    'utf8'
  )

  const inputArgv = [
    '--dir',
    root,
    '--config.registry=https://mirror.example.com/',
    '--config.//mirror.example.com/:_authToken=secret-token-123',
    '--config.lockfile-dir=shared-lf',
    '--config.store-dir=custom-store',
    '--config.strict-ssl=false',
    '--config.custom-setting=custom-val',
    'run',
    'build',
  ]

  const prevXdgConfigHome = process.env.XDG_CONFIG_HOME
  const prevArgv = process.argv
  process.env.XDG_CONFIG_HOME = xdgConfigHome
  process.argv = ['node', 'pnpm', ...inputArgv]
  try {
    const parsed = await parseCliArgs(inputArgv)
    const { config, context } = await getConfig(parsed.options, {
      excludeReporter: false,
      workspaceDir: parsed.workspaceDir,
      rawCliConfig: parsed.rawCliConfig,
      printWarnings: false,
    })

    // 1. Record / bootstrap config
    const bootstrap = getPackageManagerBootstrapConfig(config)
    expect(bootstrap.registriesByScope.default).toBe('https://mirror.example.com/')
    expect(bootstrap.configByUri?.['//mirror.example.com/']?.['@']?.authToken).toBe('secret-token-123')
    expect(bootstrap.strictSsl).toBe(false)
    expect(config.strictSsl).toBe(false)
    expect(config.lockfileDir).toBe(path.join(root, 'shared-lf'))
    expect(config.storeDir).toBe('custom-store')

    await handlePackageManagerAndRuntimes({
      cmd: parsed.cmd,
      cliOptions: parsed.options,
      config,
      context,
      rejectUnknownOptions: false,
      recordPinWithoutSwitch: true,
    })
    expect(recordedAt).toBe(path.join(root, 'shared-lf'))

    // 2. Switched pnpm receives the exact same CLI config flags
    await handlePackageManagerAndRuntimes({
      cmd: parsed.cmd,
      cliOptions: parsed.options,
      config,
      context: {
        ...context,
        wantedPackageManager: {
          name: 'pnpm',
          version: '9.1.0',
          fromDevEngines: true,
          onFail: 'download',
        },
      },
      rejectUnknownOptions: false,
      recordPinWithoutSwitch: true,
    })
    expect(switchedVersion).toBe('9.1.0')
    expect(switchedArgs).toEqual(inputArgv)

    // 3. verifyDepsBeforeRun child install args
    const verifyDepsArgs = createCliConfigArgs({
      ...config,
      ...context,
    })
    const expectedStoreDir = path.join(root, 'custom-store')
    expect(verifyDepsArgs).toEqual(expect.arrayContaining([
      '--config.registry=https://mirror.example.com/',
      '--config.//mirror.example.com/:_authToken=secret-token-123',
      `--config.lockfile-dir=${path.join(root, 'shared-lf')}`,
      `--config.store-dir=${expectedStoreDir}`,
      '--config.strict-ssl=false',
      '--config.custom-setting=custom-val',
    ]))

    const vdParsed = await parseCliArgs(['install', '--dir', root, ...verifyDepsArgs])
    const { config: vdConfig } = await getConfig(vdParsed.options, {
      excludeReporter: false,
      workspaceDir: vdParsed.workspaceDir,
      rawCliConfig: vdParsed.rawCliConfig,
      printWarnings: false,
    })
    const vdBootstrap = getPackageManagerBootstrapConfig(vdConfig)
    expect(vdBootstrap.registriesByScope.default).toBe('https://mirror.example.com/')
    expect(vdBootstrap.configByUri?.['//mirror.example.com/']?.['@']?.authToken).toBe('secret-token-123')
    expect(vdBootstrap.strictSsl).toBe(false)
    expect(vdConfig.strictSsl).toBe(false)
    expect(vdConfig.lockfileDir).toBe(path.join(root, 'shared-lf'))
    expect(vdConfig.storeDir).toBe(expectedStoreDir)
  } finally {
    process.argv = prevArgv
    if (prevXdgConfigHome === undefined) {
      delete process.env.XDG_CONFIG_HOME
    } else {
      process.env.XDG_CONFIG_HOME = prevXdgConfigHome
    }
  }
})

test('pin_recorded_only_from_full_config_at_lockfile_dir', async () => {
  const root = fs.realpathSync(tempDir(false))
  const xdgConfigHome = path.join(root, 'xdg-config')
  const globalConfigDir = path.join(xdgConfigHome, 'pnpm')
  fs.mkdirSync(globalConfigDir, { recursive: true })
  fs.writeFileSync(
    path.join(root, 'package.json'),
    JSON.stringify({
      name: 'root',
      devEngines: {
        packageManager: {
          name: 'pnpm',
          version: packageManager.version,
          onFail: 'download',
        },
      },
    }),
    'utf8'
  )

  const prevXdgConfigHome = process.env.XDG_CONFIG_HOME
  process.env.XDG_CONFIG_HOME = xdgConfigHome
  try {
    // 1. Readable full config with lockfileDir in pnpm-workspace.yaml -> records at lockfileDir
    fs.writeFileSync(
      path.join(root, 'pnpm-workspace.yaml'),
      'packages:\n  - .\nlockfileDir: custom-lf\n',
      'utf8'
    )
    recordedAt = undefined
    const readableOutcome = await runCliFlow(root, ['--dir', root, '--reporter=silent', 'root'])
    expect(readableOutcome).toEqual({ outcome: 'record', at: 'custom-lf' })

    // 2. Unreadable setting in pnpm-workspace.yaml -> does not record pin
    fs.writeFileSync(
      path.join(root, 'pnpm-workspace.yaml'),
      'packages:\n  - .\nlockfileDir: custom-lf\nremoteSideEffectsCache: 123\n',
      'utf8'
    )
    recordedAt = undefined
    const unreadableWorkspaceCmd = await runCliFlow(root, ['--dir', root, '--reporter=silent', 'root'])
    expect(unreadableWorkspaceCmd).toEqual({ outcome: 'error' })
    expect(recordedAt).toBeUndefined()

    recordedAt = undefined
    const unreadableWorkspaceVersion = await runCliFlow(root, ['--dir', root, '--reporter=silent', '--version'])
    expect(unreadableWorkspaceVersion).toEqual({ outcome: 'nothing' })
    expect(recordedAt).toBeUndefined()

    // 3. Unreadable setting in .npmrc -> does not record pin
    fs.writeFileSync(
      path.join(root, 'pnpm-workspace.yaml'),
      'packages:\n  - .\nlockfileDir: custom-lf\n',
      'utf8'
    )
    fs.writeFileSync(
      path.join(root, '.npmrc'),
      '//registry.example.com/:tokenHelper=/bin/echo s3cr3t\n',
      'utf8'
    )
    recordedAt = undefined
    const unreadableNpmrcVersion = await runCliFlow(root, ['--dir', root, '--reporter=silent', '--version'])
    expect(unreadableNpmrcVersion).toEqual({ outcome: 'nothing' })
    expect(recordedAt).toBeUndefined()
    fs.unlinkSync(path.join(root, '.npmrc'))

    // 4. Unreadable setting in global config.yaml -> does not record pin
    fs.writeFileSync(
      path.join(globalConfigDir, 'config.yaml'),
      'remoteSideEffectsCache: 123\n',
      'utf8'
    )
    recordedAt = undefined
    const unreadableGlobalVersion = await runCliFlow(root, ['--dir', root, '--reporter=silent', '--version'])
    expect(unreadableGlobalVersion).toEqual({ outcome: 'nothing' })
    expect(recordedAt).toBeUndefined()

    // 5. Corrupted pnpm-lock.yaml is skipped when the pin does not consult the lockfile
    fs.writeFileSync(path.join(globalConfigDir, 'config.yaml'), '', 'utf8')
    fs.writeFileSync(path.join(root, 'pnpm-workspace.yaml'), 'packages:\n  - .\n', 'utf8')
    fs.writeFileSync(path.join(root, 'pnpm-lock.yaml'), ': : : not valid yaml [ [ [\n', 'utf8')
    fs.writeFileSync(
      path.join(root, 'package.json'),
      JSON.stringify({
        name: 'root',
        devEngines: { packageManager: { name: 'pnpm', version: '9.1.0', onFail: 'warn' } },
      }),
      'utf8'
    )
    expect(await runCliFlow(root, ['--dir', root, '--reporter=silent', 'root'])).toEqual({ outcome: 'nothing' })
    expect(await runCliFlow(root, ['--dir', root, '--reporter=silent', '--global', 'root'])).toEqual({ outcome: 'nothing' })
  } finally {
    if (prevXdgConfigHome === undefined) {
      delete process.env.XDG_CONFIG_HOME
    } else {
      process.env.XDG_CONFIG_HOME = prevXdgConfigHome
    }
  }
})
