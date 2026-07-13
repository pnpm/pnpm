import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import type { PnpmError } from '@pnpm/error'
import { add, remove } from '@pnpm/installing.commands'
import { type LogBase, streamParser } from '@pnpm/logger'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import type { Project, ProjectManifest, ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { loadJsonFile } from 'load-json-file'
import { temporaryDirectory } from 'tempy'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const tmp = temporaryDirectory()

const DEFAULT_OPTIONS = {
  argv: {
    original: [],
  },
  bail: false,
  bin: 'node_modules/.bin',
  cacheDir: path.join(tmp, 'cache'),
  excludeLinksFromLockfile: false,
  extraEnv: {},
  cliOptions: {},
  deployAllFiles: false,
  include: {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  },
  lock: true,
  preferWorkspacePackages: true,
  pnpmfile: ['.pnpmfile.cjs'],
  pnpmHomeDir: '',
  configByUri: {},
  registries: {
    default: REGISTRY_URL,
  },
  rootProjectManifestDir: '',
  sort: true,
  storeDir: path.join(tmp, 'store'),
  userConfig: {},
  workspaceConcurrency: 1,
  virtualStoreDirMaxLength: process.platform === 'win32' ? 60 : 120,
}

const describeOnLinuxOnly = process.platform === 'linux' ? describe : describe.skip

test('installing with "workspace:" should work even if link-workspace-packages is off', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: false,
    saveWorkspaceProtocol: false,
    workspaceDir: process.cwd(),
  }, ['project-2@workspace:*'])

  const { default: pkg } = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toEqual({ 'project-2': 'workspace:^2.0.0' })

  projects['project-1'].has('project-2')
})

test('installing with "workspace:" should work even if link-workspace-packages is off and save-workspace-protocol is "rolling"', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: false,
    saveWorkspaceProtocol: 'rolling',
    workspaceDir: process.cwd(),
  }, ['project-2@workspace:*'])

  const { default: pkg } = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toEqual({ 'project-2': 'workspace:*' })

  projects['project-1'].has('project-2')
})

test('installing with "workspace=true" should work even if link-workspace-packages is off and save-workspace-protocol is false', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: false,
    saveWorkspaceProtocol: false,
    workspace: true,
    workspaceDir: process.cwd(),
  }, ['project-2'])

  const { default: pkg } = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toEqual({ 'project-2': 'workspace:^2.0.0' })

  projects['project-1'].has('project-2')
})

test('add: fail when "workspace" option is true but the command runs not in a workspace', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  let err!: PnpmError
  try {
    await add.handler({
      ...DEFAULT_OPTIONS,
      dir: path.resolve('project-1'),
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: true,
    }, ['project-2'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_WORKSPACE_OPTION_OUTSIDE_WORKSPACE')
  expect(err.message).toBe('--workspace can only be used inside a workspace')
})

test('installing with "workspace=true" with linkWorkspacePackages on and saveWorkspaceProtocol off', async () => {
  const projects = preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
    },
    {
      name: 'project-2',
      version: '2.0.0',
    },
  ])

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project-1'),
    linkWorkspacePackages: true,
    saveWorkspaceProtocol: false,
    workspace: true,
    workspaceDir: process.cwd(),
  }, ['project-2'])

  const { default: pkg } = await import(path.resolve('project-1/package.json'))

  expect(pkg?.dependencies).toEqual({ 'project-2': 'workspace:^2.0.0' })

  projects['project-1'].has('project-2')
})

test('add: fail when --no-save option is used', async () => {
  let err!: PnpmError
  try {
    await add.handler({
      ...DEFAULT_OPTIONS,
      cliOptions: {
        save: false,
      },
      dir: process.cwd(),
      linkWorkspacePackages: false,
    }, ['is-positive'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_OPTION_NOT_SUPPORTED')
  expect(err.message).toBe('The "add" command currently does not support the no-save option')
})

test('pnpm add --save-peer', async () => {
  const project = prepare()

  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
    savePeer: true,
  }, ['is-positive@1.0.0'])

  {
    const manifest = await loadJsonFile(path.resolve('package.json'))

    expect(
      manifest
    ).toStrictEqual(
      {
        name: 'project',
        version: '0.0.0',

        devDependencies: { 'is-positive': '1.0.0' },
        peerDependencies: { 'is-positive': '1.0.0' },
      }
    )
  }

  project.has('is-positive')

  await remove.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
  }, ['is-positive'])

  project.hasNot('is-positive')

  {
    const manifest = await loadJsonFile(path.resolve('package.json'))

    expect(
      manifest
    ).toStrictEqual(
      {
        name: 'project',
        version: '0.0.0',
      }
    )
  }
})

test('pnpm add - with save-prefix set to empty string should save package version without prefix', async () => {
  prepare()
  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
    savePrefix: '',
  }, ['is-positive@1.0.0'])

  {
    const manifest = await loadJsonFile(path.resolve('package.json'))

    expect(
      manifest
    ).toStrictEqual(
      {
        name: 'project',
        version: '0.0.0',
        dependencies: { 'is-positive': '1.0.0' },
      }
    )
  }
})

test('pnpm add - should add prefix when set in .npmrc when a range is not specified explicitly', async () => {
  prepare()
  await add.handler({
    ...DEFAULT_OPTIONS,
    dir: process.cwd(),
    linkWorkspacePackages: false,
    savePrefix: '~',
  }, ['is-positive'])

  {
    const { default: manifest } = (await import(path.resolve('package.json')))

    expect(
      manifest.dependencies['is-positive']
    ).toMatch(/~(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Z-]+(?:\.[0-9A-Z-]+)*))?(?:\+[0-9A-Z-]+)?$/i)
  }
})

test('pnpm add automatically installs missing peer dependencies', async () => {
  const project = prepare()
  await add.handler({
    ...DEFAULT_OPTIONS,
    autoInstallPeers: true,
    dir: process.cwd(),
    linkWorkspacePackages: false,
  }, ['@pnpm.e2e/abc@1.0.0'])

  const lockfile = project.readLockfile()
  expect(Object.keys(lockfile.packages)).toHaveLength(5)
})

test('pnpm add handles matching workspace project when dir differs from project.rootDir', async () => {
  const rootProjectManifest: ProjectManifest = {
    name: 'project',
    version: '0.0.0',
  }
  const project = prepare(rootProjectManifest)
  const rootDir = project.dir() as ProjectRootDir
  const allProjects: Project[] = [
    {
      manifest: rootProjectManifest,
      rootDir,
      rootDirRealPath: fs.realpathSync(rootDir) as ProjectRootDirRealPath,
      writeProjectManifest: async (manifest) => project.writePackageJson(manifest),
    },
  ]

  await expect(add.handler({
    ...DEFAULT_OPTIONS,
    allProjects,
    dir: `${rootDir}${path.sep}`,
    linkWorkspacePackages: false,
    lockfileDir: rootDir,
    rootProjectManifest,
    rootProjectManifestDir: rootDir,
    sharedWorkspaceLockfile: true,
    workspaceDir: rootDir,
  }, ['is-positive@1.0.0'])).resolves.toBeUndefined()

  const manifest = await loadJsonFile<ProjectManifest>(path.resolve('package.json'))
  expect(manifest.dependencies).toStrictEqual({
    'is-positive': '1.0.0',
  })
  project.has('is-positive')
})

test('add: fail when global bin directory is not found', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await add.handler({
      ...DEFAULT_OPTIONS,
      bin: undefined as any, // eslint-disable-line
      dir: path.resolve('project-1'),
      global: true,
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: true,
    }, ['@pnpm.e2e/hello-world-js-bin'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_NO_GLOBAL_BIN_DIR')
})

test('add: fail trying to install pnpm', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await add.handler({
      ...DEFAULT_OPTIONS,
      bin: path.resolve('project/bin'),
      dir: path.resolve('project'),
      global: true,
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: false,
    }, ['pnpm'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_GLOBAL_PNPM_INSTALL')
})

test('add: fail trying to install @pnpm/exe', async () => {
  prepareEmpty()

  let err!: PnpmError
  try {
    await add.handler({
      ...DEFAULT_OPTIONS,
      bin: path.resolve('project/bin'),
      dir: path.resolve('project'),
      global: true,
      linkWorkspacePackages: false,
      saveWorkspaceProtocol: false,
      workspace: false,
    }, ['@pnpm/exe'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_GLOBAL_PNPM_INSTALL')
})

test('minimumReleaseAge with minimumReleaseAgeStrict enabled makes install fail if there is no version that was published before the cutoff', async () => {
  prepareEmpty()

  const isOdd011ReleaseDate = new Date(2016, 11, 7 - 2) // 0.1.1 was released at 2016-12-07T07:18:01.205Z
  const diff = Date.now() - isOdd011ReleaseDate.getTime()
  const minimumReleaseAge = diff / (60 * 1000) // converting to minutes

  await expect(add.handler({
    ...DEFAULT_OPTIONS,
    dir: path.resolve('project'),
    minimumReleaseAge,
    minimumReleaseAgeStrict: true,
    linkWorkspacePackages: false,
  }, ['is-odd@0.1.1'])).rejects.toThrow(/is-odd@0\.1\.1 was published.+minimumReleaseAge cutoff/)
})

describeOnLinuxOnly('filters optional dependencies based on pnpm.supportedArchitectures.libc', () => {
  test.each([
    ['glibc', '@pnpm.e2e+only-linux-x64-glibc@1.0.0', '@pnpm.e2e+only-linux-x64-musl@1.0.0'],
    ['musl', '@pnpm.e2e+only-linux-x64-musl@1.0.0', '@pnpm.e2e+only-linux-x64-glibc@1.0.0'],
  ])('%p → installs %p, does not install %p', async (libc, found, notFound) => {
    const rootProjectManifest: ProjectManifest = {}

    prepare(rootProjectManifest)

    await add.handler({
      ...DEFAULT_OPTIONS,
      rootProjectManifest,
      dir: process.cwd(),
      linkWorkspacePackages: true,
      supportedArchitectures: {
        os: ['linux'],
        cpu: ['x64'],
        libc: [libc],
      },
    }, ['@pnpm.e2e/support-different-architectures'])

    const pkgDirs = fs.readdirSync(path.resolve('node_modules', '.pnpm'))
    expect(pkgDirs).toContain('@pnpm.e2e+support-different-architectures@1.0.0')
    expect(pkgDirs).toContain(found)
    expect(pkgDirs).not.toContain(notFound)
  })
})

// Captures `warn`-level messages emitted via `@pnpm/logger`'s `globalWarn`
// (and any other warn-level logger call) while `fn` runs. The license
// checker has no dedicated reporter hook, so this is the same
// streamParser-subscription mechanism `@pnpm/logger`'s own tests use to
// observe log output (see core/logger/test/index.test.ts): `globalWarn`
// writes through bole's shared output pipeline, which the singleton
// `streamParser` is already subscribed to, so no module mocking is needed.
async function captureWarnings<T> (fn: () => Promise<T>): Promise<{ result: T, warnings: string[] }> {
  const warnings: string[] = []
  const handleLog = (msg: LogBase): void => {
    if (msg.level === 'warn') {
      warnings.push(msg.message)
    }
  }
  streamParser.on('data', handleLog)
  try {
    const result = await fn()
    // bole writes synchronously, but the ndjson transform stream that feeds
    // streamParser flows its 'data' events on a later tick; give any
    // already-queued log line a chance to arrive before reading `warnings`.
    await new Promise<void>((resolve) => setImmediate(resolve))
    return { result, warnings }
  } finally {
    streamParser.removeListener('data', handleLog)
  }
}

describe('license compliance after add', () => {
  test('pnpm add fails when a disallowed license is present in strict mode', async () => {
    prepare()

    await expect(
      add.handler({
        ...DEFAULT_OPTIONS,
        dir: process.cwd(),
        linkWorkspacePackages: false,
        licenses: {
          disallowed: ['MIT'],
          mode: 'strict',
        },
      }, ['is-positive@1.0.0'])
    ).rejects.toThrow('license violation')
  })

  // Regression test for #1: shallow-mode used to derive direct deps from the
  // (possibly stale) in-memory root manifest. For a plain, non-workspace
  // `pnpm add`, no rootProjectManifest is even passed in, so the shallow
  // filter treated the project as having zero direct deps and silently
  // dropped every scanned package — including the package that was *just
  // added* by this very command. The fix derives direct-dep identities from
  // the lockfile written to disk moments earlier by this same add, so the
  // just-added package is caught.
  test('pnpm add fails on the just-added package under a shallow-mode disallowed-license policy (regression #1)', async () => {
    prepare()

    let err!: PnpmError
    try {
      await add.handler({
        ...DEFAULT_OPTIONS,
        dir: process.cwd(),
        linkWorkspacePackages: false,
        licenses: {
          disallowed: ['MIT'],
          mode: 'strict',
          depth: 'shallow',
        },
      }, ['is-positive@1.0.0'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_LICENSE_VIOLATION')
  })

  // Regression test for #5: a missing lockfile (e.g. `useLockfile: false`)
  // used to make the post-install license check return silently, so a
  // configured policy was skipped with no indication to the user. The fix
  // surfaces this via a visible `globalWarn` instead of a silent no-op; the
  // add itself must still succeed (fail-open, but no longer fail-silent).
  test('pnpm add succeeds and warns when useLockfile is false, skipping the license check (regression #5)', async () => {
    const project = prepare()

    const { warnings } = await captureWarnings(() =>
      add.handler({
        ...DEFAULT_OPTIONS,
        dir: process.cwd(),
        linkWorkspacePackages: false,
        useLockfile: false,
        licenses: {
          disallowed: ['MIT'],
          mode: 'strict',
        },
      }, ['is-positive@1.0.0'])
    )

    project.has('is-positive')
    expect(warnings.some((message) => message.includes('License check skipped'))).toBe(true)
  })

  // Regression test for #7: the fix (commit 242853781e) routes
  // `checkAfterInstall` through `scanAndCheckLicenses` and reports any
  // non-empty `result.warnings` via `globalWarn` instead of silently
  // discarding it, while letting the add succeed either way.
  //
  // `matchLicenseAgainstPolicy` now returns `allowed: false` (reason
  // `not-in-allowed-list`) for a loose-mode license that isn't in the
  // configured allowed list, instead of silently allowing it. That makes
  // this policy shape reach `result.warnings` for real, so this test
  // captures the `globalWarn` output and asserts the warning is actually
  // emitted — not just that the add doesn't fail.
  test('pnpm add succeeds and warns when a loose-mode policy does not allow-list the installed license (regression #7)', async () => {
    const project = prepare()

    const { warnings } = await captureWarnings(() =>
      add.handler({
        ...DEFAULT_OPTIONS,
        dir: process.cwd(),
        linkWorkspacePackages: false,
        licenses: {
          allowed: ['MIT'],
          mode: 'loose',
        },
      }, ['@pnpm.e2e/has-different-licenses@2.0.0'])
    )

    project.has('@pnpm.e2e/has-different-licenses')
    // has-different-licenses@2.0.0 is licensed ISC, which is not in the
    // configured `allowed: ['MIT']` list, so it must surface as a license
    // warning while still letting the add succeed.
    expect(warnings.some((message) =>
      message.includes('license warning') &&
      message.includes('has-different-licenses') &&
      message.includes('ISC')
    )).toBe(true)
  })

  test('pnpm add succeeds when licenses.mode is none', async () => {
    const project = prepare()

    await add.handler({
      ...DEFAULT_OPTIONS,
      dir: process.cwd(),
      linkWorkspacePackages: false,
      licenses: {
        disallowed: ['MIT'],
        mode: 'none',
      },
    }, ['is-positive@1.0.0'])

    project.has('is-positive')
  })

  test('pnpm add succeeds when no licenses config is set', async () => {
    const project = prepare()

    await add.handler({
      ...DEFAULT_OPTIONS,
      dir: process.cwd(),
      linkWorkspacePackages: false,
    }, ['is-positive@1.0.0'])

    project.has('is-positive')
  })

  // Regression test for the post-install license check on rootless workspaces.
  // preparePackages creates the workspace root without a package.json; recursive
  // add must not require the root manifest to scan licenses.
  test('recursive pnpm add succeeds on a rootless workspace with licenses.mode set', async () => {
    const projects = preparePackages([
      {
        name: 'project-1',
        version: '1.0.0',
      },
      {
        name: 'project-2',
        version: '1.0.0',
      },
    ])

    const { allProjects, selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

    await add.handler({
      ...DEFAULT_OPTIONS,
      allProjects,
      dir: process.cwd(),
      linkWorkspacePackages: false,
      recursive: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
      licenses: {
        mode: 'loose',
      },
    }, ['is-positive@1.0.0'])

    projects['project-1'].has('is-positive')
    projects['project-2'].has('is-positive')
  })

  // Regression test: `add --config` used to `return` before ever reaching
  // `runLicenseCheck`, so it silently bypassed the license policy entirely.
  // It doesn't scan the configDependency itself (configDependencies live in
  // a separate env-lockfile document, outside the manifest.dependencies
  // graph the scanner walks), but it must still enforce the policy against
  // the project's existing regular dependencies instead of skipping the
  // check outright.
  test('pnpm add --config still runs the post-install license check', async () => {
    const project = prepare()

    // Install a disallowed-license dependency first, with no license policy
    // configured, so a violation already exists in the project.
    await add.handler({
      ...DEFAULT_OPTIONS,
      dir: process.cwd(),
      linkWorkspacePackages: false,
    }, ['is-positive@1.0.0'])

    project.has('is-positive')

    await expect(
      add.handler({
        ...DEFAULT_OPTIONS,
        dir: process.cwd(),
        rootProjectManifestDir: process.cwd(),
        linkWorkspacePackages: false,
        config: true,
        licenses: {
          disallowed: ['MIT'],
          mode: 'strict',
        },
      }, ['@pnpm.e2e/foo@100.0.0'])
    ).rejects.toThrow('license violation')
  })
})

// Global installs (`pnpm add -g`) split each param into its own isolated
// install group under a global package dir (own lockfile + manifest +
// node_modules), with no scannable project at the global dir root. These
// tests exercise the full, unmocked handleGlobalAdd → installGlobalPackages
// → runLicenseCheckForGlobalInstall chain to prove the per-group license
// check resolves the right (absolute) store/virtual-store paths for a
// directory that isn't the process cwd, and that a violation cleans up the
// half-applied install group instead of leaving it linked.
describe('license compliance after global add', () => {
  test('pnpm add -g fails when a disallowed license is present in strict mode, and cleans up the install group', async () => {
    prepare()
    const globalDir = path.resolve('..', 'global')
    const bin = path.join(globalDir, 'bin')
    const globalPkgDir = path.join(globalDir, 'pnpm')

    let err!: PnpmError
    try {
      await add.handler({
        ...DEFAULT_OPTIONS,
        dir: process.cwd(),
        global: true,
        bin,
        globalPkgDir,
        linkWorkspacePackages: false,
        licenses: {
          disallowed: ['MIT'],
          mode: 'strict',
        },
      }, ['is-positive@1.0.0'])
    } catch (_err: any) { // eslint-disable-line
      err = _err
    }
    expect(err.code).toBe('ERR_PNPM_LICENSE_VIOLATION')

    // The violating group's install dir must be removed so it isn't left
    // half-applied: no install dir/hash symlink remains under the global
    // package dir, and the bin dir is never even created (bin linking
    // happens after the license check).
    expect(fs.readdirSync(globalPkgDir)).toHaveLength(0)
    expect(fs.existsSync(bin)).toBe(false)
  })

  test('pnpm add -g succeeds and links the bin when the license is allowed', async () => {
    prepare()
    const globalDir = path.resolve('..', 'global')
    const bin = path.join(globalDir, 'bin')
    const globalPkgDir = path.join(globalDir, 'pnpm')

    await add.handler({
      ...DEFAULT_OPTIONS,
      dir: process.cwd(),
      global: true,
      bin,
      globalPkgDir,
      linkWorkspacePackages: false,
      licenses: {
        disallowed: ['ISC'],
        mode: 'strict',
      },
    }, ['@pnpm.e2e/hello-world-js-bin@1.0.0'])

    expect(fs.existsSync(path.join(bin, 'hello-world-js-bin'))).toBe(true)

    const [hashEntry] = fs.readdirSync(globalPkgDir)
    const installDir = fs.realpathSync(path.join(globalPkgDir, hashEntry))
    expect(fs.existsSync(path.join(installDir, 'node_modules', '@pnpm.e2e', 'hello-world-js-bin'))).toBe(true)
  })
})
