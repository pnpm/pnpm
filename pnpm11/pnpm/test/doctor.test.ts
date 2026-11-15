import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import http from 'node:http'
import type { AddressInfo } from 'node:net'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { STORE_VERSION } from '@pnpm/constants'
import { isError } from '@pnpm/error'
import { prepare } from '@pnpm/prepare'

import { type CheckResult, handler, type LinkOperations } from '../src/cmd/doctor.js'
import { canWriteToDir, checkFilesystemCapabilities } from '../src/cmd/doctorFilesystem.js'
import { canWriteWindowsDirSecurity, isSafeWindowsDirSecurity } from '../src/cmd/doctorWindowsAcl.js'
import { execPnpmSync } from './utils/execPnpm.js'
import { closeServer, listenOnLocalhost } from './utils/localServer.js'

const PNPM_BIN = path.resolve(import.meta.dirname, '../bin/pnpm.mjs')

function simulatedMount (filePath: string): string | undefined {
  const realParent = fs.realpathSync(path.dirname(filePath))
  return realParent.split(path.sep).find((part) => part.startsWith('mount-'))
}

function createCrossMountLinkOperations (): LinkOperations {
  const assertSameMount = (source: string, dest: string): void => {
    if (simulatedMount(source) !== simulatedMount(dest)) {
      throw Object.assign(new Error('EXDEV: cross-device link not permitted'), { code: 'EXDEV' })
    }
  }
  return {
    reflink (source, dest) {
      try {
        assertSameMount(source, dest)
        fs.copyFileSync(source, dest)
        return true
      } catch {
        return false
      }
    },
    async hardlink (source, dest) {
      assertSameMount(source, dest)
      await fs.promises.link(source, dest)
    },
  }
}

test('pnpm doctor pings the configured default registry with its credentials', async () => {
  prepare()
  const authorizationHeaders: Array<string | undefined> = []
  const server = http.createServer((req, res) => {
    authorizationHeaders.push(req.headers.authorization)
    res.end('{}')
  })
  try {
    await listenOnLocalhost(server)
    const { port } = server.address() as AddressInfo
    const registry = `http://127.0.0.1:${port}/`
    const { output } = await handler({
      dir: process.cwd(),
      cacheDir: path.resolve('cache'),
      pnpmHomeDir: path.resolve('pnpm-home'),
      registriesByScope: { default: registry },
      configByUri: { [`//127.0.0.1:${port}/`]: { '@': { authToken: 'secret' } } },
      json: true,
      pnpmCommand: [process.execPath, '-e', ''],
    })
    const { checks } = JSON.parse(output) as { checks: CheckResult[] }
    const connectivity = checks.find((check) => check.title === 'Registry connectivity')

    expect(connectivity?.status).toBe('pass')
    expect(connectivity?.detail).toContain(registry)
    expect(authorizationHeaders).toStrictEqual(['Bearer secret'])
  } finally {
    await closeServer(server)
  }
})

test('pnpm doctor fails the connectivity check when the configured registry responds with an error', async () => {
  prepare()
  const server = http.createServer((_req, res) => {
    res.statusCode = 500
    res.end()
  })
  try {
    await listenOnLocalhost(server)
    const { port } = server.address() as AddressInfo
    const registry = `http://127.0.0.1:${port}/`
    const { output, exitCode } = await handler({
      dir: process.cwd(),
      cacheDir: path.resolve('cache'),
      pnpmHomeDir: path.resolve('pnpm-home'),
      registriesByScope: { default: registry },
      configByUri: {},
      json: true,
      pnpmCommand: [process.execPath, '-e', ''],
    })
    const { checks } = JSON.parse(output) as { checks: CheckResult[] }
    const connectivity = checks.find((check) => check.title === 'Registry connectivity')

    expect(connectivity?.status).toBe('fail')
    expect(connectivity?.detail).toContain(registry)
    expect(exitCode).toBe(1)
  } finally {
    await closeServer(server)
  }
})

test('pnpm doctor succeeds on a fresh environment before cacheDir, storeDir, or pnpmHomeDir exist', async () => {
  prepare()
  const root = process.cwd()
  const projectDir = path.join(root, 'proj')
  const cacheDir = path.join(root, 'fresh-cache', 'pnpm')
  const pnpmHomeDir = path.join(root, 'fresh-pnpm-home')
  const storeDir = path.join(pnpmHomeDir, 'store', 'v10')
  fs.mkdirSync(projectDir, { recursive: true })

  const originalPath = process.env.PATH
  const originalCorepack = process.env.COREPACK_ROOT
  process.env.PATH = [pnpmHomeDir, originalPath].filter(Boolean).join(path.delimiter)
  delete process.env.COREPACK_ROOT
  try {
    const { output, exitCode } = await handler({
      dir: projectDir,
      cacheDir,
      pnpmHomeDir,
      storeDir,
      offline: true,
      json: true,
      registriesByScope: { default: 'https://registry.npmjs.org/' },
      configByUri: {},
      pnpmCommand: [process.execPath, PNPM_BIN],
    })
    const { checks } = JSON.parse(output) as { checks: CheckResult[] }

    expect(exitCode).toBe(0)
    for (const title of [
      'Global bin directory',
      'Cache directory',
      'Store directory',
      'Filesystem',
      'Registry connectivity',
      'Install smoke test',
    ]) {
      expect(checks.find((check) => check.title === title)?.status).toBe('pass')
    }
    expect(fs.existsSync(path.join(root, 'fresh-cache'))).toBe(false)
    expect(fs.existsSync(pnpmHomeDir)).toBe(false)
    expect(fs.existsSync(path.join(projectDir, 'node_modules'))).toBe(false)
  } finally {
    process.env.PATH = originalPath
    if (originalCorepack !== undefined) {
      process.env.COREPACK_ROOT = originalCorepack
    }
  }
})

test('pnpm doctor fails store write check and warns on filesystem check when storeDir is blocked by a file', async () => {
  prepare()
  const root = process.cwd()
  const blocker = path.join(root, 'not-a-dir')
  fs.writeFileSync(blocker, 'file')

  const { output, exitCode } = await handler({
    dir: root,
    cacheDir: path.join(root, 'cache'),
    pnpmHomeDir: path.join(root, 'pnpm-home'),
    storeDir: path.join(blocker, 'store'),
    offline: true,
    json: true,
    registriesByScope: { default: 'https://registry.npmjs.org/' },
    configByUri: {},
    pnpmCommand: [process.execPath, PNPM_BIN],
  })
  const { checks } = JSON.parse(output) as { checks: CheckResult[] }
  const storeCheck = checks.find((check) => check.title === 'Store directory')
  const fsCheck = checks.find((check) => check.title === 'Filesystem')

  expect(exitCode).toBe(1)
  expect(storeCheck?.status).toBe('fail')
  expect(storeCheck?.detail).toContain('no write access to ')
  expect(fsCheck?.status).toBe('warn')
  expect(fsCheck?.detail).toBe('could not create a probe directory')
})

test('pnpm doctor warns when storeDir and project are on different filesystems', async () => {
  prepare()
  const root = process.cwd()
  const mountA = path.join(root, 'mount-a')
  const mountB = path.join(root, 'mount-b')
  const projectDir = path.join(mountB, 'proj')
  fs.mkdirSync(mountA, { recursive: true })
  fs.mkdirSync(projectDir, { recursive: true })

  const { output, exitCode } = await handler({
    dir: projectDir,
    cacheDir: path.join(mountA, 'cache'),
    pnpmHomeDir: path.join(mountA, 'pnpm-home'),
    storeDir: path.join(mountA, 'store'),
    offline: true,
    json: true,
    registriesByScope: { default: 'https://registry.npmjs.org/' },
    configByUri: {},
    pnpmCommand: [process.execPath, PNPM_BIN],
    linkOperations: createCrossMountLinkOperations(),
  })
  const { checks } = JSON.parse(output) as { checks: CheckResult[] }
  const fsCheck = checks.find((check) => check.title === 'Filesystem')

  expect(exitCode).toBe(0)
  expect(fsCheck?.status).toBe('warn')
  if (process.platform !== 'win32') {
    expect(fsCheck?.detail).toBe('available: symlink')
  }
  expect(fsCheck?.fix).toBe(
    'Neither reflink nor hardlink works between the store and this project; installs will copy files and be slower. Put the store on the same filesystem as your projects.'
  )
})

test('pnpm doctor CLI reads storeDir from pnpm-workspace.yaml and checks the versioned store path', () => {
  prepare()
  const root = process.cwd()
  const blocker = path.join(root, 'blocked-store')
  fs.writeFileSync(blocker, 'file')
  fs.writeFileSync(path.join(root, 'pnpm-workspace.yaml'), 'storeDir: ./blocked-store\n')

  const result = execPnpmSync(['doctor', '--offline', '--json'], {
    cwd: root,
    omitEnvDefaults: ['pnpm_config_store_dir'],
  })
  expect(result.status).toBe(1)
  const { checks } = JSON.parse(result.stdout.toString()) as { checks: CheckResult[] }
  const storeCheck = checks.find((check) => check.title === 'Store directory')
  expect(storeCheck?.status).toBe('fail')
  expect(storeCheck?.detail).toContain(path.join('blocked-store', STORE_VERSION))
})

test('pnpm doctor probes the versioned store directory and hoisted lockfile root modules directory', async () => {
  prepare()
  const root = process.cwd()
  const storeBase = path.join(root, 'shared-store')
  const mountAVersionedStore = path.join(root, 'mount-a', 'store-v')
  const memberDir = path.join(root, 'mount-a', 'member')
  const workspaceRoot = path.join(root, 'mount-b', 'workspace')
  fs.mkdirSync(storeBase, { recursive: true })
  fs.mkdirSync(mountAVersionedStore, { recursive: true })
  fs.mkdirSync(memberDir, { recursive: true })
  fs.mkdirSync(workspaceRoot, { recursive: true })
  fs.symlinkSync(mountAVersionedStore, path.join(storeBase, STORE_VERSION), 'junction')

  const fsCheck = await checkFilesystemCapabilities({
    dir: memberDir,
    workspaceDir: workspaceRoot,
    lockfileDir: workspaceRoot,
    nodeLinker: 'hoisted',
    pnpmHomeDir: path.join(workspaceRoot, 'pnpm-home'),
    storeDir: storeBase,
    linkOperations: createCrossMountLinkOperations(),
  })

  expect(fsCheck.status).toBe('warn')
})

test('pnpm doctor honors per-project packageConfigs.modulesDir when lockfileDir is unset', async () => {
  prepare()
  const root = process.cwd()
  const mountA = path.join(root, 'mount-a')
  const mountB = path.join(root, 'mount-b')
  const memberDir = path.join(mountA, 'member')
  fs.mkdirSync(memberDir, { recursive: true })
  fs.mkdirSync(mountB, { recursive: true })
  fs.writeFileSync(path.join(memberDir, 'package.json'), JSON.stringify({ name: 'member', version: '1.0.0' }))

  const fsCheck = await checkFilesystemCapabilities({
    dir: memberDir,
    workspaceDir: mountA,
    pnpmHomeDir: path.join(mountA, 'pnpm-home'),
    storeDir: path.join(mountA, 'store'),
    packageConfigs: { member: { modulesDir: path.join(mountB, 'custom-modules') } },
    linkOperations: createCrossMountLinkOperations(),
  })

  expect(fsCheck.status).toBe('warn')
})

test('pnpm doctor resolves the linkable mount root .pnpm-store fallback when storeDir is unset', async () => {
  prepare()
  const root = process.cwd()
  const mountA = path.join(root, 'mount-a')
  const mountB = path.join(root, 'mount-b')
  const projectDir = path.join(mountB, 'workspace', 'proj')
  fs.mkdirSync(mountA, { recursive: true })
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(mountB, '.pnpm-store'), 'blocked')

  const fsCheck = await checkFilesystemCapabilities({
    dir: projectDir,
    pnpmHomeDir: path.join(mountA, 'pnpm-home'),
    linkOperations: createCrossMountLinkOperations(),
  })

  expect(fsCheck).toStrictEqual({
    title: 'Filesystem',
    status: 'warn',
    detail: 'could not create a probe directory',
    fix: 'Check that the store and project directories are writable.',
  })
  expect(fs.readdirSync(mountA)).toStrictEqual([])
  expect(fs.readdirSync(mountB).sort()).toStrictEqual(['.pnpm-store', 'workspace'])
  expect(fs.readdirSync(projectDir)).toStrictEqual([])
})

test('pnpm doctor keeps the home store target when storeDir is unset and pnpmHomeDir/store is blocked by a file', async () => {
  prepare()
  const root = process.cwd()
  const pnpmHomeDir = path.join(root, 'pnpm-home')
  const projectDir = path.join(root, 'proj')
  fs.mkdirSync(pnpmHomeDir, { recursive: true })
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(pnpmHomeDir, 'store'), 'blocked')

  const fsCheck = await checkFilesystemCapabilities({
    dir: projectDir,
    pnpmHomeDir,
    linkOperations: createCrossMountLinkOperations(),
  })

  expect(fsCheck).toStrictEqual({
    title: 'Filesystem',
    status: 'warn',
    detail: 'could not create a probe directory',
    fix: 'Check that the store and project directories are writable.',
  })
  expect(fs.readdirSync(pnpmHomeDir)).toStrictEqual(['store'])
  expect(fs.readdirSync(projectDir)).toStrictEqual([])
})

test('pnpm doctor passes filesystem check across mounts when global virtual store links on the store mount', async () => {
  prepare()
  const root = process.cwd()
  const mountA = path.join(root, 'mount-a')
  const projectDir = path.join(root, 'mount-b', 'proj')
  fs.mkdirSync(mountA, { recursive: true })
  fs.mkdirSync(projectDir, { recursive: true })

  const fsCheck = await checkFilesystemCapabilities({
    dir: projectDir,
    pnpmHomeDir: path.join(mountA, 'pnpm-home'),
    storeDir: path.join(mountA, 'store'),
    enableGlobalVirtualStore: true,
    linkOperations: createCrossMountLinkOperations(),
  })

  expect(fsCheck.status).toBe('pass')
})

test('pnpm doctor returns a filesystem warning when writing the store probe file fails', async () => {
  prepare()
  const root = process.cwd()
  const projectDir = path.join(root, 'proj')
  const storeDir = path.join(root, 'store', STORE_VERSION)
  fs.mkdirSync(projectDir, { recursive: true })
  fs.mkdirSync(storeDir, { recursive: true })

  const realOpen = fs.promises.open.bind(fs.promises)
  const openSpy = jest.spyOn(fs.promises, 'open').mockImplementation(async (target, flags, mode) => {
    if (typeof target === 'string' && target.includes('.pnpm-source-')) {
      throw Object.assign(new Error('ENOSPC: no space left on device'), { code: 'ENOSPC' })
    }
    return realOpen(target, flags, mode)
  })
  try {
    const { output, exitCode } = await handler({
      dir: projectDir,
      cacheDir: path.join(root, 'cache'),
      pnpmHomeDir: path.join(root, 'pnpm-home'),
      storeDir,
      offline: true,
      json: true,
      registriesByScope: { default: 'https://registry.npmjs.org/' },
      configByUri: {},
      pnpmCommand: [process.execPath, '-e', ''],
    })
    const { checks } = JSON.parse(output) as { checks: CheckResult[] }
    const fsCheck = checks.find((check) => check.title === 'Filesystem')

    expect(exitCode).toBe(1)
    expect(fsCheck).toStrictEqual({
      title: 'Filesystem',
      status: 'warn',
      detail: 'could not write a probe file',
      fix: 'Check that the store directory is writable.',
    })
    expect(fs.readdirSync(storeDir)).toStrictEqual([])
    expect(fs.readdirSync(projectDir)).toStrictEqual([])
  } finally {
    openSpy.mockRestore()
  }
})

test('pnpm doctor refuses to write probe files beneath a world-writable or group-writable non-sticky ancestor', async () => {
  if (process.platform === 'win32') return
  prepare()
  const root = process.cwd()
  for (const mode of [0o777, 0o775]) {
    const unsafeParent = path.join(root, `unsafe-${mode.toString(8)}`)
    const projectDir = path.join(unsafeParent, 'proj')
    fs.mkdirSync(projectDir, { recursive: true })
    fs.chmodSync(unsafeParent, mode)
    try {
      const fsCheck = await checkFilesystemCapabilities({ // eslint-disable-line no-await-in-loop -- sequential chmod test cases
        dir: projectDir,
        pnpmHomeDir: path.join(root, 'pnpm-home'),
      })

      expect(fsCheck).toStrictEqual({
        title: 'Filesystem',
        status: 'warn',
        detail: 'cannot safely probe link support in a directory writable by other users',
        fix: 'Check ownership and permissions on the store and project ancestor directories.',
      })
      expect(fs.readdirSync(projectDir)).toStrictEqual([])
    } finally {
      fs.chmodSync(unsafeParent, 0o755)
    }
  }
})

test('pnpm doctor returns unsafe warning rather than falling back to .pnpm-store when storeDir is unset and pnpmHomeDir is unsafe', async () => {
  if (process.platform === 'win32') return
  prepare()
  const root = process.cwd()
  const unsafeParent = path.join(root, 'shared-home-parent')
  const pnpmHomeDir = path.join(unsafeParent, 'pnpm-home')
  const projectDir = path.join(root, 'proj')
  fs.mkdirSync(pnpmHomeDir, { recursive: true })
  fs.mkdirSync(projectDir, { recursive: true })
  fs.writeFileSync(path.join(pnpmHomeDir, 'store'), 'blocked')
  fs.chmodSync(unsafeParent, 0o777)
  try {
    const fsCheck = await checkFilesystemCapabilities({
      dir: projectDir,
      pnpmHomeDir,
    })

    expect(fsCheck).toStrictEqual({
      title: 'Filesystem',
      status: 'warn',
      detail: 'cannot safely probe link support in a directory writable by other users',
      fix: 'Check ownership and permissions on the store and project ancestor directories.',
    })
    expect(fs.readdirSync(projectDir)).toStrictEqual([])
  } finally {
    fs.chmodSync(unsafeParent, 0o755)
  }
})

test('pnpm doctor refuses to write probe files beneath a shared Windows ancestor', async () => {
  if (process.platform !== 'win32') return
  prepare()
  const root = process.cwd()
  const sharedParent = path.join(root, 'shared-everyone')
  const projectDir = path.join(sharedParent, 'proj')
  fs.mkdirSync(projectDir, { recursive: true })
  execFileSync('icacls', [sharedParent, '/grant', '*S-1-1-0:(OI)(CI)(F)'])
  try {
    const fsCheck = await checkFilesystemCapabilities({
      dir: projectDir,
      pnpmHomeDir: path.join(root, 'pnpm-home'),
    })

    expect(fsCheck).toStrictEqual({
      title: 'Filesystem',
      status: 'warn',
      detail: 'cannot safely probe link support in a directory writable by other users',
      fix: 'Check ownership and permissions on the store and project ancestor directories.',
    })
    expect(fs.readdirSync(projectDir)).toStrictEqual([])
  } finally {
    execFileSync('icacls', [sharedParent, '/reset', '/T'])
  }
})

test('Windows directory security evaluator distinguishes standard roots from shared directories', () => {
  const me = 'S-1-5-21-10-20-30-1001'
  const admins = new Set(['S-1-5-21-10-20-30-1000'])

  expect(isSafeWindowsDirSecurity({
    ok: true,
    nullAcl: false,
    owner: 'S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464',
    rules: [
      { sid: 'S-1-5-18', allow: true, mask: 0x001f01ff, inherit: 3, prop: 0 },
      { sid: 'S-1-5-32-544', allow: true, mask: 0x001f01ff, inherit: 3, prop: 0 },
      { sid: 'S-1-3-0', allow: true, mask: 0x10000000, inherit: 3, prop: 2 },
      { sid: 'S-1-5-32-545', allow: true, mask: 0x001200a9, inherit: 3, prop: 0 },
      { sid: 'S-1-5-32-545', allow: true, mask: -1610612736, inherit: 3, prop: 2 },
      { sid: 'S-1-5-32-545', allow: true, mask: 0x00000004, inherit: 1, prop: 0 },
      { sid: 'S-1-5-32-545', allow: true, mask: 0x00000002, inherit: 1, prop: 2 },
    ],
  }, me, admins)).toBe(true)

  expect(isSafeWindowsDirSecurity({
    ok: true,
    nullAcl: false,
    owner: me,
    rules: [
      { sid: 'S-1-5-32-545', allow: true, mask: 0x001301bf, inherit: 3, prop: 0 },
    ],
  }, me, admins)).toBe(false)

  expect(isSafeWindowsDirSecurity({
    ok: true,
    nullAcl: false,
    owner: 'S-1-5-21-10-20-30-1002',
    rules: [
      { sid: 'S-1-5-18', allow: true, mask: 0x001f01ff, inherit: 3, prop: 0 },
    ],
  }, me, admins)).toBe(false)

  expect(isSafeWindowsDirSecurity({
    ok: true,
    nullAcl: true,
    owner: me,
    rules: [{ sid: 'S-1-1-0', allow: true, mask: 0x001f01ff, inherit: 3, prop: 0 }],
  }, me, admins)).toBe(false)

  expect(canWriteWindowsDirSecurity({
    ok: true,
    nullAcl: false,
    rules: [
      { sid: 'S-1-1-0', allow: true, mask: 0x00000006, inherit: 3, prop: 0 },
      { sid: me, allow: false, mask: 0x00000002, inherit: 3, prop: 0 },
    ],
  }, new Set([me, 'S-1-1-0']))).toBe(false)
})

test('canWriteToDir verifies file creation in an existing directory even if subdirectory creation works', () => {
  prepare()
  const existingDir = path.resolve('existing-dir')
  fs.mkdirSync(existingDir, { recursive: true })

  const openSpy = jest.spyOn(fs, 'openSync').mockImplementation(() => {
    throw Object.assign(new Error('EACCES: permission denied'), { code: 'EACCES' })
  })
  try {
    expect(canWriteToDir(existingDir)).toBe(false)
  } finally {
    openSpy.mockRestore()
  }
  expect(canWriteToDir(existingDir)).toBe(true)
  expect(fs.readdirSync(existingDir)).toStrictEqual([])
})

test('canWriteToDir rejects empty, file-blocked, dangling-symlink, and read-only ancestor paths', () => {
  prepare()
  const root = process.cwd()
  const blocker = path.join(root, 'blocker-file')
  fs.writeFileSync(blocker, 'file')
  expect(canWriteToDir('')).toBe(false)
  expect(canWriteToDir(blocker)).toBe(false)
  expect(canWriteToDir(path.join(blocker, 'cache', 'pnpm'))).toBe(false)

  const dangling = path.join(root, 'dangling-link')
  let createdSymlink = false
  try {
    fs.symlinkSync(path.join(root, 'nowhere'), dangling, 'dir')
    createdSymlink = true
  } catch (err: unknown) {
    if (!(isError(err) && 'code' in err && err.code === 'EPERM' && process.platform === 'win32')) {
      throw err
    }
  }
  if (createdSymlink) {
    expect(canWriteToDir(dangling)).toBe(false)
    expect(canWriteToDir(path.join(dangling, 'pnpm'))).toBe(false)
  }

  if (process.platform !== 'win32' && process.getuid?.() !== 0) {
    const readonlyDir = path.join(root, 'readonly-dir')
    fs.mkdirSync(readonlyDir, { mode: 0o555 })
    try {
      expect(canWriteToDir(readonlyDir)).toBe(false)
      expect(canWriteToDir(path.join(readonlyDir, 'missing', 'store'))).toBe(false)
    } finally {
      fs.chmodSync(readonlyDir, 0o755)
    }
  }
})
