import { existsSync, promises as fs } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

import { afterEach, expect, jest, test } from '@jest/globals'
import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import type { GlobalPackageInfo } from '@pnpm/global.packages'
import type { DependencyManifest } from '@pnpm/types'

type LinkBinsOfPackages = typeof import('@pnpm/bins.linker').linkBinsOfPackages
type RemoveBin = typeof import('@pnpm/bins.remover').removeBin
type SymlinkDir = typeof import('symlink-dir').symlinkDir

let testRoot: string | undefined
let linkFailure: { afterWrites: number, error: Error } | undefined
let activationLinkFailure: Error | undefined
let restorationLinkFailure: Error | undefined
let freshCleanupFailure: { path: string, error: Error } | undefined
let removeBinFailure: { name: string, error: Error } | undefined
let backupRemovalFailure: Error | undefined
let obstructBackupCleanup = false
let skipMissingBinSources = false
let symlinkCallCount = 0
const linkedBinNames: string[] = []
const backupSymlinkTypes: Array<string | null | undefined> = []
const activationBackupFileContents: Buffer[] = []
const getHashLink = jest.fn((globalDir: string, hash: string) => path.join(globalDir, hash))
const getInstalledBinNames = jest.fn<(pkg: GlobalPackageInfo) => Promise<string[]>>()
const globalWarn = jest.fn<(message: string) => void>()
const realRm = fs.rm.bind(fs)

jest.spyOn(fs, 'rm').mockImplementation(async (target, options) => {
  const failure = freshCleanupFailure
  if (failure != null && path.resolve(String(target)) === failure.path) {
    freshCleanupFailure = undefined
    throw failure.error
  }
  if (backupRemovalFailure != null && path.basename(String(target)).startsWith('.pnpm-bin-backup-')) {
    const err = backupRemovalFailure
    backupRemovalFailure = undefined
    throw err
  }
  await realRm(target, options)
})

const linkBinsOfPackages = jest.fn<LinkBinsOfPackages>(async (pkgs, globalBinDir, opts = {}) => {
  const commands = (await Promise.all(pkgs.map(async ({ manifest, location }) => {
    return (await getBinsFromPackageManifest(manifest, location)).map((command) => ({
      command,
      pkgName: manifest.name,
    }))
  })))
    .flat()
    .filter(({ command }) => !opts.excludeBins?.has(command.name))

  await fs.mkdir(globalBinDir, { recursive: true })
  const writtenPkgNames: string[] = []
  /* eslint-disable no-await-in-loop -- sequential writes make the injected partial failure deterministic */
  for (const { command, pkgName } of commands) {
    if (skipMissingBinSources && !existsSync(command.path)) continue
    const slot = path.join(globalBinDir, command.name)
    await fs.rm(slot, { force: true, recursive: true })
    await fs.copyFile(command.path, slot)
    const sourceStat = await fs.stat(command.path)
    await fs.chmod(slot, sourceStat.mode & 0o777)
    linkedBinNames.push(command.name)
    writtenPkgNames.push(pkgName)
    if (linkFailure != null && writtenPkgNames.length === linkFailure.afterWrites) {
      throw linkFailure.error
    }
  }
  /* eslint-enable no-await-in-loop */
  return writtenPkgNames
})

const removeBin = jest.fn<RemoveBin>(async (cmd) => {
  if (removeBinFailure != null && path.basename(cmd) === removeBinFailure.name) {
    throw removeBinFailure.error
  }
  const extensions = process.platform === 'win32' ? ['', '.cmd', '.ps1', '.exe'] : ['']
  await Promise.all(extensions.map(async (extension) => fs.rm(`${cmd}${extension}`, { force: true, recursive: true })))
})

// Both the activation swap and the rollback swap go through this; the
// call order decides which injected failure fires.
async function onHashLinkSwap (): Promise<void> {
  symlinkCallCount++
  if (symlinkCallCount === 1) {
    if (testRoot == null) throw new Error('Expected a activation fixture before linking the hash directory')
    const backupDirs = await findBackupDirs(testRoot)
    activationBackupFileContents.push(...(await Promise.all(backupDirs.map(readRegularFileContents))).flat())
    if (obstructBackupCleanup) {
      const [backupDir] = backupDirs
      if (backupDir == null) throw new Error('Expected a global bin backup directory')
      await fs.writeFile(path.join(backupDir, 'cleanup-obstruction'), 'keep backup directory non-empty\n')
    }
  }
  if (symlinkCallCount === 2 && restorationLinkFailure != null) {
    throw restorationLinkFailure
  }
  if (symlinkCallCount === 1 && activationLinkFailure != null) {
    throw activationLinkFailure
  }
}

// Used only on the Windows path, where the swap cannot be a rename.
const symlinkDir = jest.fn<SymlinkDir>(async (target, linkPath) => {
  await onHashLinkSwap()
  await replaceDirectorySymlink(target, linkPath)
  return { reused: false }
})

const realSymlink = fs.symlink.bind(fs)
jest.spyOn(fs, 'symlink').mockImplementation(async (target, linkPath, type) => {
  // The staged link is the POSIX hash-link swap; everything else is
  // fixture seeding or a bin-slot backup.
  if (String(linkPath).endsWith('.tmp')) {
    await onHashLinkSwap()
  } else if (String(linkPath).includes(`${path.sep}.pnpm-bin-backup-`)) {
    backupSymlinkTypes.push(type)
  }
  await realSymlink(target, linkPath, type)
})

jest.unstable_mockModule('@pnpm/bins.linker', () => ({ linkBinsOfPackages }))
jest.unstable_mockModule('@pnpm/bins.remover', () => ({ removeBin }))
jest.unstable_mockModule('@pnpm/global.packages', () => ({ getHashLink, getInstalledBinNames }))
jest.unstable_mockModule('@pnpm/logger', () => ({ globalWarn }))
jest.unstable_mockModule('symlink-dir', () => ({ symlinkDir }))

const { cleanupReplacedGlobalInstalls, activateGlobalInstall } = await import('../src/globalActivation.js')

afterEach(async () => {
  const root = testRoot
  testRoot = undefined
  freshCleanupFailure = undefined
  try {
    if (root != null) await fs.rm(root, { force: true, recursive: true })
  } finally {
    linkFailure = undefined
    activationLinkFailure = undefined
    restorationLinkFailure = undefined
    backupRemovalFailure = undefined
    obstructBackupCleanup = false
    removeBinFailure = undefined
    skipMissingBinSources = false
    symlinkCallCount = 0
    backupSymlinkTypes.length = 0
    linkedBinNames.length = 0
    activationBackupFileContents.length = 0
    getHashLink.mockClear()
    getInstalledBinNames.mockReset()
    globalWarn.mockClear()
    linkBinsOfPackages.mockClear()
    removeBin.mockClear()
    symlinkDir.mockClear()
  }
})

test('restores exact bin slots when linking fails after a partial write', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      executable: 'bin/executable.js',
      linked: 'bin/linked.js',
    },
  }
  const fixture = await createFixture(manifest)
  const executableSlot = path.join(fixture.globalBinDir, 'executable')
  const oldExecutableBytes = Buffer.from('#!/bin/sh\necho old executable\n')
  await fs.writeFile(executableSlot, oldExecutableBytes)
  await fs.chmod(executableSlot, 0o751)
  const executableBefore = await readSlotState(executableSlot)

  const linkedSlot = path.join(fixture.globalBinDir, 'linked')
  const linkedTarget = path.join(fixture.oldInstallDir, 'linked-target.js')
  await fs.writeFile(linkedTarget, 'old linked target\n')
  await fs.chmod(linkedTarget, 0o754)
  const linkedBefore = await seedSymlinkOrRegularFile(linkedSlot, linkedTarget)

  const activationError = new Error('linker stopped after one write')
  linkFailure = { afterWrites: 1, error: activationError }

  await expect(activateGlobalInstall({
    installDir: fixture.freshInstallDir,
    hashLink: fixture.hashLink,
    globalBinDir: fixture.globalBinDir,
    pkgs: [{ manifest, location: fixture.packageDir }],
    binsToSkip: new Set(),
  })).rejects.toBe(activationError)

  expect(linkedBinNames).toStrictEqual(['executable'])
  expect(await readSlotState(executableSlot)).toStrictEqual(executableBefore)
  const restoredLinked = await readSlotState(linkedSlot)
  if (linkedBefore.symlinkSupported) {
    expect(restoredLinked.kind).toBe('symlink')
  } else {
    expect(restoredLinked.kind).toBe('file')
  }
  expect(restoredLinked).toStrictEqual(linkedBefore.state)
  expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.oldInstallDir))
  expect(existsSync(fixture.oldInstallDir)).toBe(true)
  expect(existsSync(fixture.freshInstallDir)).toBe(false)
  expect(await findBackupDirs(fixture.root)).toStrictEqual([])
})

test('leaves skipped bins untouched when hash-link activation fails', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      replacement: 'bin/replacement.js',
      shared: 'bin/shared.js',
    },
  }
  const fixture = await createFixture(manifest)
  const replacementSlot = path.join(fixture.globalBinDir, 'replacement')
  const oldReplacementBytes = Buffer.from('old replacement\n')
  await fs.writeFile(replacementSlot, oldReplacementBytes)
  await fs.chmod(replacementSlot, 0o750)
  const replacementBefore = await readSlotState(replacementSlot)
  const sharedSlot = path.join(fixture.globalBinDir, 'shared')
  const skippedSharedBytes = Buffer.from('other package owns this slot\n')
  await fs.writeFile(sharedSlot, skippedSharedBytes)
  await fs.chmod(sharedSlot, 0o740)
  const sharedBefore = await readSlotState(sharedSlot)
  const activationError = new Error('hash-link activation failed')
  activationLinkFailure = activationError

  await expect(activateGlobalInstall({
    installDir: fixture.freshInstallDir,
    hashLink: fixture.hashLink,
    globalBinDir: fixture.globalBinDir,
    pkgs: [{ manifest, location: fixture.packageDir }],
    binsToSkip: new Set(['shared']),
  })).rejects.toBe(activationError)

  // The hash link is the switch-over, so a failure there aborts before any
  // bin is linked.
  expect(linkedBinNames).toStrictEqual([])
  expect(await readSlotState(replacementSlot)).toStrictEqual(replacementBefore)
  expect(await readSlotState(sharedSlot)).toStrictEqual(sharedBefore)
  expect(activationBackupFileContents).toContainEqual(oldReplacementBytes)
  expect(activationBackupFileContents).not.toContainEqual(skippedSharedBytes)
  expect(removeBin).toHaveBeenCalledWith(replacementSlot)
  expect(removeBin).not.toHaveBeenCalledWith(sharedSlot)
  expect(symlinkCallCount).toBe(2)
  expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.oldInstallDir))
  expect(existsSync(fixture.oldInstallDir)).toBe(true)
  expect(existsSync(fixture.freshInstallDir)).toBe(false)
  expect(await findBackupDirs(fixture.root)).toStrictEqual([])
})

test('keeps recovery artifacts when rollback fails', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      replacement: 'bin/replacement.js',
    },
  }
  const fixture = await createFixture(manifest)
  const replacementSlot = path.join(fixture.globalBinDir, 'replacement')
  await fs.writeFile(replacementSlot, 'old replacement\n')
  await fs.chmod(replacementSlot, 0o750)
  const replacementBefore = await readSlotState(replacementSlot)
  const activationError = new Error('hash-link activation failed')
  activationLinkFailure = activationError
  restorationLinkFailure = new Error('hash-link restoration failed')

  let thrown: unknown
  try {
    await activateGlobalInstall({
      installDir: fixture.freshInstallDir,
      hashLink: fixture.hashLink,
      globalBinDir: fixture.globalBinDir,
      pkgs: [{ manifest, location: fixture.packageDir }],
      binsToSkip: new Set(),
    })
  } catch (err) {
    thrown = err
  }

  expect(util.types.isNativeError(thrown)).toBe(true)
  if (!util.types.isNativeError(thrown)) throw new Error('Expected activateGlobalInstall to throw a native error')
  expect(thrown).toMatchObject({
    code: 'ERR_PNPM_GLOBAL_BIN_ROLLBACK_FAILED',
  })
  expect(thrown.cause).toBe(activationError)
  expect(await readSlotState(replacementSlot)).toStrictEqual(replacementBefore)
  const backupDirs = await findBackupDirs(fixture.root)
  expect(backupDirs).toHaveLength(1)
  expect(thrown.message).toContain(backupDirs[0])
  expect(thrown.message).toContain(fixture.freshInstallDir)
  expect(existsSync(backupDirs[0])).toBe(true)
  expect(existsSync(fixture.freshInstallDir)).toBe(true)
  expect(symlinkCallCount).toBe(2)
})

test('preserves Windows file and directory symlink kinds with relative targets', async () => {
  const platform = Object.getOwnPropertyDescriptor(process, 'platform')
  if (platform == null) throw new Error('Expected process.platform to be an own property')
  Object.defineProperty(process, 'platform', { ...platform, value: 'win32' })
  try {
    const manifest: DependencyManifest = {
      name: 'replacement',
      version: '2.0.0',
      bin: {
        'file-link': 'bin/file-link.js',
        'dir-link': 'bin/dir-link.js',
      },
    }
    const fixture = await createFixture(manifest)
    const fileTarget = path.join('..', 'old-install', 'file-target.js')
    const dirTarget = path.join('..', 'old-install', 'dir-target')
    await fs.writeFile(path.join(fixture.oldInstallDir, 'file-target.js'), 'old file target\n')
    await fs.mkdir(path.join(fixture.oldInstallDir, 'dir-target'))
    const fileLink = path.join(fixture.globalBinDir, 'file-link')
    const dirLink = path.join(fixture.globalBinDir, 'dir-link')
    if (!await seedSymlinkOrSkip(fileTarget, fileLink, 'file')) return
    if (!await seedSymlinkOrSkip(dirTarget, dirLink, 'dir')) return
    const activationError = new Error('hash-link activation failed')
    activationLinkFailure = activationError

    await expect(activateGlobalInstall({
      installDir: fixture.freshInstallDir,
      hashLink: fixture.hashLink,
      globalBinDir: fixture.globalBinDir,
      pkgs: [{ manifest, location: fixture.packageDir }],
      binsToSkip: new Set(),
    })).rejects.toBe(activationError)

    expect(backupSymlinkTypes).toHaveLength(2)
    expect(new Set(backupSymlinkTypes)).toStrictEqual(new Set(['file', 'dir']))
    expect(await fs.readlink(fileLink)).toBe(fileTarget)
    expect(await fs.readlink(dirLink)).toBe(dirTarget)
    expect((await fs.stat(fileLink)).isFile()).toBe(true)
    expect((await fs.stat(dirLink)).isDirectory()).toBe(true)
  } finally {
    Object.defineProperty(process, 'platform', platform)
  }
})

test('links bins through the hash link, and moves it before linking', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      tool: 'bin/tool.js',
    },
  }
  const fixture = await createFixture(manifest)

  await activateGlobalInstall({
    installDir: fixture.freshInstallDir,
    hashLink: fixture.hashLink,
    globalBinDir: fixture.globalBinDir,
    pkgs: [{ manifest, location: fixture.packageDir }],
    binsToSkip: new Set(),
  })

  // A shim embeds the path it was generated from. Generating it from the
  // hash link is what lets the next update switch the command over by
  // moving that link alone, leaving the shim byte-identical.
  const [linkedPkgs] = linkBinsOfPackages.mock.calls[0]
  const relativeLocation = path.relative(fixture.freshInstallDir, fixture.packageDir)
  expect(linkedPkgs[0].location).toBe(path.join(fixture.hashLink, relativeLocation))
  expect(linkedPkgs[0].location.startsWith(fixture.freshInstallDir)).toBe(false)
  expect(symlinkCallCount).toBe(1)
})

test('activates into a global bin directory that does not exist yet', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      tool: 'bin/tool.js',
    },
  }
  const fixture = await createFixture(manifest)
  await fs.rm(fixture.globalBinDir, { recursive: true })

  const activatedBins = await activateGlobalInstall({
    installDir: fixture.freshInstallDir,
    hashLink: fixture.hashLink,
    globalBinDir: fixture.globalBinDir,
    pkgs: [{ manifest, location: fixture.packageDir }],
    binsToSkip: new Set(),
  })

  expect(activatedBins).toStrictEqual(new Set(['tool']))
  expect(await fs.readFile(path.join(fixture.globalBinDir, 'tool'), 'utf8')).toBe('fresh tool\n')
  expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.freshInstallDir))
  expect(await findBackupDirs(fixture.root)).toStrictEqual([])
})

test('succeeds when removing the backup directory fails after activation', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      tool: 'bin/tool.js',
    },
  }
  const fixture = await createFixture(manifest)
  const toolSlot = path.join(fixture.globalBinDir, 'tool')
  await fs.writeFile(toolSlot, 'old tool\n')
  const backupRemovalMessage = 'backup directory removal failed'
  backupRemovalFailure = new Error(backupRemovalMessage)

  const activatedBins = await activateGlobalInstall({
    installDir: fixture.freshInstallDir,
    hashLink: fixture.hashLink,
    globalBinDir: fixture.globalBinDir,
    pkgs: [{ manifest, location: fixture.packageDir }],
    binsToSkip: new Set(),
  })

  expect(activatedBins).toStrictEqual(new Set(['tool']))
  expect(await fs.readFile(toolSlot, 'utf8')).toBe('fresh tool\n')
  expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.freshInstallDir))
  expect(await findBackupDirs(fixture.root)).toHaveLength(1)
  // Committed activation must not fail, but the leak has to be visible.
  expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining('Failed to remove the global bin backup directory'))
  expect(globalWarn).toHaveBeenCalledWith(expect.stringContaining(backupRemovalMessage))
})

test('activates when a Windows bin slot is a dangling symlink', async () => {
  const platform = Object.getOwnPropertyDescriptor(process, 'platform')
  if (platform == null) throw new Error('Expected process.platform to be an own property')
  Object.defineProperty(process, 'platform', { ...platform, value: 'win32' })
  try {
    const manifest: DependencyManifest = {
      name: 'replacement',
      version: '2.0.0',
      bin: {
        tool: 'bin/tool.js',
      },
    }
    const fixture = await createFixture(manifest)
    const toolSlot = path.join(fixture.globalBinDir, 'tool')
    if (!await seedSymlinkOrSkip(path.join(fixture.oldInstallDir, 'missing-target.js'), toolSlot, 'file')) return

    const activatedBins = await activateGlobalInstall({
      installDir: fixture.freshInstallDir,
      hashLink: fixture.hashLink,
      globalBinDir: fixture.globalBinDir,
      pkgs: [{ manifest, location: fixture.packageDir }],
      binsToSkip: new Set(),
    })

    expect(activatedBins).toStrictEqual(new Set(['tool']))
    expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.freshInstallDir))
    expect(await findBackupDirs(fixture.root)).toStrictEqual([])
  } finally {
    Object.defineProperty(process, 'platform', platform)
  }
})

test('rejects an unsupported bin slot type and cleans up preparation artifacts', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      tool: 'bin/tool.js',
    },
  }
  const fixture = await createFixture(manifest)
  const toolSlot = path.join(fixture.globalBinDir, 'tool')
  await fs.mkdir(toolSlot)

  await expect(activateGlobalInstall({
    installDir: fixture.freshInstallDir,
    hashLink: fixture.hashLink,
    globalBinDir: fixture.globalBinDir,
    pkgs: [{ manifest, location: fixture.packageDir }],
    binsToSkip: new Set(),
  })).rejects.toMatchObject({
    code: 'ERR_PNPM_GLOBAL_BIN_UNSUPPORTED_TYPE',
    message: expect.stringContaining(toolSlot),
  })

  expect((await fs.lstat(toolSlot)).isDirectory()).toBe(true)
  expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.oldInstallDir))
  expect(existsSync(fixture.freshInstallDir)).toBe(false)
  expect(await findBackupDirs(fixture.root)).toStrictEqual([])
})

test('reports fresh-install cleanup failure without claiming a core rollback failure', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      replacement: 'bin/replacement.js',
    },
  }
  const fixture = await createFixture(manifest)
  const replacementSlot = path.join(fixture.globalBinDir, 'replacement')
  await fs.writeFile(replacementSlot, 'old replacement\n')
  const replacementBefore = await readSlotState(replacementSlot)
  const activationError = new Error('hash-link activation failed')
  const cleanupError = new Error('fresh-install cleanup failed')
  activationLinkFailure = activationError
  freshCleanupFailure = { path: fixture.freshInstallDir, error: cleanupError }

  let thrown: unknown
  try {
    await activateGlobalInstall({
      installDir: fixture.freshInstallDir,
      hashLink: fixture.hashLink,
      globalBinDir: fixture.globalBinDir,
      pkgs: [{ manifest, location: fixture.packageDir }],
      binsToSkip: new Set(),
    })
  } catch (err) {
    thrown = err
  }

  expect(util.types.isNativeError(thrown)).toBe(true)
  if (!util.types.isNativeError(thrown) || !('errors' in thrown) || !Array.isArray(thrown.errors)) {
    throw new Error('Expected an aggregate cleanup error')
  }
  expect(thrown.name).toBe('AggregateError')
  expect('code' in thrown ? thrown.code : undefined).toBeUndefined()
  expect(thrown.cause).toBe(activationError)
  expect(thrown.errors).toEqual(expect.arrayContaining([activationError, cleanupError]))
  expect(thrown.message).toContain(fixture.freshInstallDir)
  expect(await readSlotState(replacementSlot)).toStrictEqual(replacementBefore)
  expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.oldInstallDir))
  expect(await findBackupDirs(fixture.root)).toStrictEqual([])
  expect(existsSync(fixture.freshInstallDir)).toBe(true)
})

test('reports backup cleanup failure after restoring bins and removes the fresh install', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      replacement: 'bin/replacement.js',
    },
  }
  const fixture = await createFixture(manifest)
  const replacementSlot = path.join(fixture.globalBinDir, 'replacement')
  await fs.writeFile(replacementSlot, 'old replacement\n')
  const replacementBefore = await readSlotState(replacementSlot)
  const activationError = new Error('hash-link activation failed')
  activationLinkFailure = activationError
  obstructBackupCleanup = true

  let thrown: unknown
  try {
    await activateGlobalInstall({
      installDir: fixture.freshInstallDir,
      hashLink: fixture.hashLink,
      globalBinDir: fixture.globalBinDir,
      pkgs: [{ manifest, location: fixture.packageDir }],
      binsToSkip: new Set(),
    })
  } catch (err) {
    thrown = err
  }

  expect(util.types.isNativeError(thrown)).toBe(true)
  if (!util.types.isNativeError(thrown) || !('errors' in thrown) || !Array.isArray(thrown.errors)) {
    throw new Error('Expected an aggregate cleanup error')
  }
  expect(thrown.name).toBe('AggregateError')
  expect('code' in thrown ? thrown.code : undefined).toBeUndefined()
  expect(thrown.cause).toBe(activationError)
  expect(thrown.errors).toContain(activationError)
  const backupDirs = await findBackupDirs(fixture.root)
  expect(backupDirs).toHaveLength(1)
  expect(thrown.message).toContain(backupDirs[0])
  expect(await readSlotState(replacementSlot)).toStrictEqual(replacementBefore)
  expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.oldInstallDir))
  expect(existsSync(backupDirs[0])).toBe(true)
  expect(existsSync(fixture.freshInstallDir)).toBe(false)
})

test('preserves both cleanup errors when backup and fresh-install cleanup fail', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      replacement: 'bin/replacement.js',
    },
  }
  const fixture = await createFixture(manifest)
  const replacementSlot = path.join(fixture.globalBinDir, 'replacement')
  await fs.writeFile(replacementSlot, 'old replacement\n')
  const activationError = new Error('hash-link activation failed')
  const freshCleanupError = new Error('fresh-install cleanup failed')
  activationLinkFailure = activationError
  obstructBackupCleanup = true
  freshCleanupFailure = { path: fixture.freshInstallDir, error: freshCleanupError }

  let thrown: unknown
  try {
    await activateGlobalInstall({
      installDir: fixture.freshInstallDir,
      hashLink: fixture.hashLink,
      globalBinDir: fixture.globalBinDir,
      pkgs: [{ manifest, location: fixture.packageDir }],
      binsToSkip: new Set(),
    })
  } catch (err) {
    thrown = err
  }

  expect(util.types.isNativeError(thrown)).toBe(true)
  if (!util.types.isNativeError(thrown) || !('errors' in thrown) || !Array.isArray(thrown.errors)) {
    throw new Error('Expected an aggregate cleanup error')
  }
  expect(thrown.name).toBe('AggregateError')
  expect(thrown.errors).toHaveLength(3)
  expect(thrown.errors).toEqual(expect.arrayContaining([activationError, freshCleanupError]))
  const backupCleanupErrors = thrown.errors.filter((error) => {
    return error !== activationError && error !== freshCleanupError
  })
  expect(backupCleanupErrors).toHaveLength(1)
  expect(util.types.isNativeError(backupCleanupErrors[0])).toBe(true)
  const backupDirs = await findBackupDirs(fixture.root)
  expect(backupDirs).toHaveLength(1)
  expect(thrown.message).toContain(backupDirs[0])
  expect(thrown.message).toContain(fixture.freshInstallDir)
  expect(existsSync(backupDirs[0])).toBe(true)
  expect(existsSync(fixture.freshInstallDir)).toBe(true)
})

test('removes an old bin slot when the linker skips a missing source', async () => {
  const manifest: DependencyManifest = {
    name: 'replacement',
    version: '2.0.0',
    bin: {
      tool: 'bin/tool.js',
    },
  }
  const fixture = await createFixture(manifest)
  const toolSlot = path.join(fixture.globalBinDir, 'tool')
  await fs.writeFile(toolSlot, `#!/bin/sh\n${fixture.oldInstallDir}/bin/tool.js\n`)
  await fs.rm(path.join(fixture.packageDir, 'bin/tool.js'))
  skipMissingBinSources = true
  getInstalledBinNames.mockResolvedValue(['tool'])

  const activatedBins = await activateGlobalInstall({
    installDir: fixture.freshInstallDir,
    hashLink: fixture.hashLink,
    globalBinDir: fixture.globalBinDir,
    pkgs: [{ manifest, location: fixture.packageDir }],
    binsToSkip: new Set(),
  })
  await cleanupReplacedGlobalInstalls({
    groups: [{
      dependencies: { replacement: '1.0.0' },
      hash: 'hash-link',
      installDir: fixture.oldInstallDir,
    }],
    globalDir: fixture.root,
    globalBinDir: fixture.globalBinDir,
    activeHash: 'hash-link',
    activatedBins,
    protectedBins: new Set(),
  })

  expect(activatedBins).toStrictEqual(new Set(['tool']))
  await expect(fs.lstat(toolSlot)).rejects.toMatchObject({ code: 'ENOENT' })
  expect(existsSync(fixture.oldInstallDir)).toBe(false)
  expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.freshInstallDir))
})

test('preserves bin slots owned by the activated and surviving groups', async () => {
  const { globalDir, globalBinDir, oldInstallDir } = await createCleanupFixture()
  const activatedSlot = path.join(globalBinDir, 'activated')
  const protectedSlot = path.join(globalBinDir, 'protected')
  await Promise.all([
    fs.writeFile(activatedSlot, 'activated\n'),
    fs.writeFile(protectedSlot, 'protected\n'),
  ])
  getInstalledBinNames.mockResolvedValue(['activated', 'protected'])

  await cleanupReplacedGlobalInstalls({
    groups: [{ dependencies: { old: '1.0.0' }, hash: 'active-hash', installDir: oldInstallDir }],
    globalDir,
    globalBinDir,
    activeHash: 'active-hash',
    activatedBins: new Set(['activated']),
    protectedBins: new Set(['protected']),
  })

  expect(await fs.readFile(activatedSlot, 'utf8')).toBe('activated\n')
  expect(await fs.readFile(protectedSlot, 'utf8')).toBe('protected\n')
})

test('removes stale bins and the old install without removing the active hash link', async () => {
  const { globalDir, globalBinDir, oldInstallDir } = await createCleanupFixture()
  const activeInstallDir = path.join(globalDir, 'active-install')
  const hashLink = path.join(globalDir, 'active-hash')
  await fs.mkdir(activeInstallDir, { recursive: true })
  const staleSlot = path.join(globalBinDir, 'stale')
  await fs.writeFile(staleSlot, 'stale\n')
  await replaceDirectorySymlink(activeInstallDir, hashLink)
  getInstalledBinNames.mockResolvedValue(['stale'])

  await cleanupReplacedGlobalInstalls({
    groups: [{ dependencies: { old: '1.0.0' }, hash: 'active-hash', installDir: oldInstallDir }],
    globalDir,
    globalBinDir,
    activeHash: 'active-hash',
    activatedBins: new Set(),
    protectedBins: new Set(),
  })

  await expect(fs.lstat(staleSlot)).rejects.toMatchObject({ code: 'ENOENT' })
  expect(existsSync(oldInstallDir)).toBe(false)
  expect(await fs.realpath(hashLink)).toBe(await fs.realpath(activeInstallDir))
})

test('drops the hash link of a group replaced by a different package set, keeping its relinked bins', async () => {
  const { globalDir, globalBinDir, oldInstallDir } = await createCleanupFixture()
  const oldHashLink = path.join(globalDir, 'old-hash')
  await replaceDirectorySymlink(oldInstallDir, oldHashLink)
  // `shared` is provided by the replaced group and by the group that just
  // took its place; `dropped` only by the replaced one.
  const sharedSlot = path.join(globalBinDir, 'shared')
  const droppedSlot = path.join(globalBinDir, 'dropped')
  await Promise.all([
    fs.writeFile(sharedSlot, 'relinked at the new hash\n'),
    fs.writeFile(droppedSlot, 'dropped\n'),
  ])
  getInstalledBinNames.mockResolvedValue(['shared', 'dropped'])

  await cleanupReplacedGlobalInstalls({
    groups: [{ dependencies: { old: '1.0.0' }, hash: 'old-hash', installDir: oldInstallDir }],
    globalDir,
    globalBinDir,
    activeHash: 'new-hash',
    activatedBins: new Set(['shared']),
    protectedBins: new Set(),
  })

  // Changing the set of packages changes the hash, so `shared` was rewritten
  // to point at the new one just before this ran — unlinking the group it
  // used to belong to must not take it away again.
  expect(await fs.readFile(sharedSlot, 'utf8')).toBe('relinked at the new hash\n')
  await expect(fs.lstat(droppedSlot)).rejects.toMatchObject({ code: 'ENOENT' })
  expect(existsSync(oldHashLink)).toBe(false)
  expect(existsSync(oldInstallDir)).toBe(false)
})

test('does not delete an install directory outside the global directory', async () => {
  const { root, globalDir, globalBinDir } = await createCleanupFixture()
  const outsideInstallDir = path.join(root, 'outside-install')
  await fs.mkdir(outsideInstallDir, { recursive: true })
  const marker = path.join(outsideInstallDir, 'marker')
  await fs.writeFile(marker, 'outside\n')
  getInstalledBinNames.mockResolvedValue([])

  await cleanupReplacedGlobalInstalls({
    groups: [{ dependencies: { old: '1.0.0' }, hash: 'active-hash', installDir: outsideInstallDir }],
    globalDir,
    globalBinDir,
    activeHash: 'active-hash',
    activatedBins: new Set(),
    protectedBins: new Set(),
  })

  expect(await fs.readFile(marker, 'utf8')).toBe('outside\n')
})

test('keeps a replaced group whose bin names cannot be enumerated', async () => {
  const { globalDir, globalBinDir, oldInstallDir } = await createCleanupFixture()
  const hashLink = path.join(globalDir, 'old-hash')
  await replaceDirectorySymlink(oldInstallDir, hashLink)
  const enumerationError = new Error('cannot read the installed manifest')
  getInstalledBinNames.mockRejectedValue(enumerationError)

  await expect(cleanupReplacedGlobalInstalls({
    groups: [{ dependencies: { old: '1.0.0' }, hash: 'old-hash', installDir: oldInstallDir }],
    globalDir,
    globalBinDir,
    activeHash: 'active-hash',
    activatedBins: new Set(),
    protectedBins: new Set(),
  })).rejects.toBe(enumerationError)

  expect(existsSync(oldInstallDir)).toBe(true)
  expect(existsSync(hashLink)).toBe(true)
})

test('cleanup removes the other bins but keeps a group whose bin removal failed', async () => {
  const { globalDir, globalBinDir, oldInstallDir } = await createCleanupFixture()
  const blockedSlot = path.join(globalBinDir, 'blocked')
  const staleSlot = path.join(globalBinDir, 'stale')
  await Promise.all([
    fs.writeFile(blockedSlot, 'blocked\n'),
    fs.writeFile(staleSlot, 'stale\n'),
  ])
  getInstalledBinNames.mockResolvedValue(['blocked', 'stale'])
  const removalError = new Error('bin removal failed')
  removeBinFailure = { name: 'blocked', error: removalError }

  await expect(cleanupReplacedGlobalInstalls({
    groups: [{ dependencies: { old: '1.0.0' }, hash: 'old-hash', installDir: oldInstallDir }],
    globalDir,
    globalBinDir,
    activeHash: 'active-hash',
    activatedBins: new Set(),
    protectedBins: new Set(),
  })).rejects.toBe(removalError)

  await expect(fs.lstat(staleSlot)).rejects.toMatchObject({ code: 'ENOENT' })
  // The bin that could not be removed is only discoverable through the
  // group's manifests, so the group has to outlive the failure.
  expect(existsSync(blockedSlot)).toBe(true)
  expect(existsSync(oldInstallDir)).toBe(true)
})

interface ActivationFixture {
  root: string
  globalBinDir: string
  oldInstallDir: string
  freshInstallDir: string
  hashLink: string
  packageDir: string
}

type SlotState =
  | { kind: 'file', bytes: Buffer, mode: number }
  | { kind: 'symlink', target: string, mode: number }

async function createFixture (manifest: DependencyManifest): Promise<ActivationFixture> {
  if (typeof manifest.bin !== 'object' || manifest.bin == null) {
    throw new Error('The activation fixture requires an explicit bin map')
  }
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-global-activation-'))
  testRoot = root
  const globalBinDir = path.join(root, 'global-bin')
  const oldInstallDir = path.join(root, 'old-install')
  const freshInstallDir = path.join(root, 'fresh-install')
  const hashLink = path.join(root, 'hash-link')
  const packageDir = path.join(freshInstallDir, 'node_modules', manifest.name)
  await Promise.all([
    fs.mkdir(globalBinDir, { recursive: true }),
    fs.mkdir(oldInstallDir, { recursive: true }),
    fs.mkdir(packageDir, { recursive: true }),
  ])
  await fs.writeFile(path.join(oldInstallDir, 'marker'), 'old install\n')
  await Promise.all(Object.entries(manifest.bin).map(async ([binName, binPath]) => {
    const source = path.join(packageDir, binPath)
    await fs.mkdir(path.dirname(source), { recursive: true })
    await fs.writeFile(source, `fresh ${binName}\n`)
    await fs.chmod(source, 0o755)
  }))
  await replaceDirectorySymlink(oldInstallDir, hashLink)
  return { root, globalBinDir, oldInstallDir, freshInstallDir, hashLink, packageDir }
}

interface CleanupFixture {
  root: string
  globalDir: string
  globalBinDir: string
  oldInstallDir: string
}

async function createCleanupFixture (): Promise<CleanupFixture> {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-global-cleanup-'))
  testRoot = root
  const globalDir = path.join(root, 'global')
  const globalBinDir = path.join(root, 'bin')
  const oldInstallDir = path.join(globalDir, 'old-install')
  await Promise.all([
    fs.mkdir(globalBinDir, { recursive: true }),
    fs.mkdir(oldInstallDir, { recursive: true }),
  ])
  return { root, globalDir, globalBinDir, oldInstallDir }
}

// A Windows host without Developer Mode or elevation cannot create
// symlinks; the tests that need one bail out instead of failing.
async function seedSymlinkOrSkip (target: string, linkPath: string, type: 'file' | 'dir'): Promise<boolean> {
  try {
    await fs.symlink(target, linkPath, type)
    return true
  } catch (err) {
    const code = util.types.isNativeError(err) && 'code' in err ? String(err.code) : undefined
    if (!['EACCES', 'ENOSYS', 'ENOTSUP', 'EPERM'].includes(code ?? '')) throw err
    return false
  }
}

async function seedSymlinkOrRegularFile (
  slot: string,
  target: string
): Promise<{ state: SlotState, symlinkSupported: boolean }> {
  try {
    const linkTarget = process.platform === 'win32' ? target : path.relative(path.dirname(slot), target)
    await fs.symlink(linkTarget, slot, 'file')
    return { state: await readSlotState(slot), symlinkSupported: true }
  } catch (err) {
    const code = util.types.isNativeError(err) && 'code' in err ? String(err.code) : undefined
    if (!['EACCES', 'ENOSYS', 'ENOTSUP', 'EPERM'].includes(code ?? '')) throw err
    await fs.rm(slot, { force: true })
    await fs.writeFile(slot, 'symlinks unavailable on this host\n')
    await fs.chmod(slot, 0o754)
    return { state: await readSlotState(slot), symlinkSupported: false }
  }
}

async function readSlotState (slot: string): Promise<SlotState> {
  const stat = await fs.lstat(slot)
  const mode = stat.mode & 0o777
  if (stat.isSymbolicLink()) {
    return { kind: 'symlink', target: await fs.readlink(slot), mode }
  }
  if (stat.isFile()) {
    return { kind: 'file', bytes: await fs.readFile(slot), mode }
  }
  throw new Error(`Unexpected bin slot type at ${slot}`)
}

async function replaceDirectorySymlink (target: string, linkPath: string): Promise<void> {
  await fs.rm(linkPath, { force: true, recursive: true })
  await fs.mkdir(path.dirname(linkPath), { recursive: true })
  const linkTarget = process.platform === 'win32' ? target : path.relative(path.dirname(linkPath), target)
  await fs.symlink(linkTarget, linkPath, process.platform === 'win32' ? 'junction' : 'dir')
}

async function findBackupDirs (root: string): Promise<string[]> {
  const directories = (await fs.readdir(root, { withFileTypes: true })).filter((entry) => entry.isDirectory())
  return (await Promise.all(directories.map(async (entry) => {
    const entryPath = path.join(root, entry.name)
    return [
      ...(entry.name.startsWith('.pnpm-bin-backup-') ? [entryPath] : []),
      ...await findBackupDirs(entryPath),
    ]
  }))).flat()
}

async function readRegularFileContents (root: string): Promise<Buffer[]> {
  return (await Promise.all((await fs.readdir(root, { withFileTypes: true })).map(async (entry) => {
    const entryPath = path.join(root, entry.name)
    if (entry.isDirectory()) return readRegularFileContents(entryPath)
    if (entry.isFile()) return [await fs.readFile(entryPath)]
    return []
  }))).flat()
}
