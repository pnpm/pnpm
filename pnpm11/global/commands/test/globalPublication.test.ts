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
let publicationLinkFailure: Error | undefined
let restorationLinkFailure: Error | undefined
let freshCleanupFailure: { path: string, error: Error } | undefined
let obstructBackupCleanup = false
let skipMissingBinSources = false
let symlinkCallCount = 0
const linkedBinNames: string[] = []
const publicationBackupFileContents: Buffer[] = []
const getHashLink = jest.fn((globalDir: string, hash: string) => path.join(globalDir, hash))
const getInstalledBinNames = jest.fn<(pkg: GlobalPackageInfo) => Promise<string[]>>()
const realRm = fs.rm.bind(fs)

jest.spyOn(fs, 'rm').mockImplementation(async (target, options) => {
  const failure = freshCleanupFailure
  if (failure != null && path.resolve(String(target)) === failure.path) {
    freshCleanupFailure = undefined
    throw failure.error
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
  let writes = 0
  /* eslint-disable no-await-in-loop -- sequential writes make the injected partial failure deterministic */
  for (const { command } of commands) {
    if (skipMissingBinSources && !existsSync(command.path)) continue
    const slot = path.join(globalBinDir, command.name)
    await fs.rm(slot, { force: true, recursive: true })
    await fs.copyFile(command.path, slot)
    const sourceStat = await fs.stat(command.path)
    await fs.chmod(slot, sourceStat.mode & 0o777)
    linkedBinNames.push(command.name)
    writes++
    if (linkFailure != null && writes === linkFailure.afterWrites) {
      throw linkFailure.error
    }
  }
  /* eslint-enable no-await-in-loop */
  return commands.map(({ pkgName }) => pkgName)
})

const removeBin = jest.fn<RemoveBin>(async (cmd) => {
  const extensions = process.platform === 'win32' ? ['', '.cmd', '.ps1', '.exe'] : ['']
  await Promise.all(extensions.map(async (extension) => fs.rm(`${cmd}${extension}`, { force: true, recursive: true })))
})

const symlinkDir = jest.fn<SymlinkDir>(async (target, linkPath) => {
  symlinkCallCount++
  if (symlinkCallCount === 1) {
    if (testRoot == null) throw new Error('Expected a publication fixture before linking the hash directory')
    const backupDirs = await findBackupDirs(testRoot)
    publicationBackupFileContents.push(...(await Promise.all(backupDirs.map(readRegularFileContents))).flat())
    if (obstructBackupCleanup) {
      const [backupDir] = backupDirs
      if (backupDir == null) throw new Error('Expected a global bin backup directory')
      await fs.writeFile(path.join(backupDir, 'cleanup-obstruction'), 'keep backup directory non-empty\n')
    }
  }
  if (symlinkCallCount === 2 && restorationLinkFailure != null) {
    throw restorationLinkFailure
  }
  await replaceDirectorySymlink(target, linkPath)
  if (symlinkCallCount === 1 && publicationLinkFailure != null) {
    throw publicationLinkFailure
  }
  return { reused: false }
})

jest.unstable_mockModule('@pnpm/bins.linker', () => ({ linkBinsOfPackages }))
jest.unstable_mockModule('@pnpm/bins.remover', () => ({ removeBin }))
jest.unstable_mockModule('@pnpm/global.packages', () => ({ getHashLink, getInstalledBinNames }))
jest.unstable_mockModule('symlink-dir', () => ({ symlinkDir }))

const { cleanupReplacedGlobalInstalls, publishGlobalInstall } = await import('../src/globalPublication.js')

afterEach(async () => {
  const root = testRoot
  testRoot = undefined
  freshCleanupFailure = undefined
  try {
    if (root != null) await fs.rm(root, { force: true, recursive: true })
  } finally {
    linkFailure = undefined
    publicationLinkFailure = undefined
    restorationLinkFailure = undefined
    obstructBackupCleanup = false
    skipMissingBinSources = false
    symlinkCallCount = 0
    linkedBinNames.length = 0
    publicationBackupFileContents.length = 0
    getHashLink.mockClear()
    getInstalledBinNames.mockReset()
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

  const publicationError = new Error('linker stopped after one write')
  linkFailure = { afterWrites: 1, error: publicationError }

  await expect(publishGlobalInstall({
    installDir: fixture.freshInstallDir,
    hashLink: fixture.hashLink,
    globalBinDir: fixture.globalBinDir,
    pkgs: [{ manifest, location: fixture.packageDir }],
    binsToSkip: new Set(),
  })).rejects.toBe(publicationError)

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

test('leaves skipped bins untouched when hash-link publication fails', async () => {
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
  const publicationError = new Error('hash-link publication failed')
  publicationLinkFailure = publicationError

  await expect(publishGlobalInstall({
    installDir: fixture.freshInstallDir,
    hashLink: fixture.hashLink,
    globalBinDir: fixture.globalBinDir,
    pkgs: [{ manifest, location: fixture.packageDir }],
    binsToSkip: new Set(['shared']),
  })).rejects.toBe(publicationError)

  expect(linkedBinNames).toStrictEqual(['replacement'])
  expect(await readSlotState(replacementSlot)).toStrictEqual(replacementBefore)
  expect(await readSlotState(sharedSlot)).toStrictEqual(sharedBefore)
  expect(publicationBackupFileContents).toContainEqual(oldReplacementBytes)
  expect(publicationBackupFileContents).not.toContainEqual(skippedSharedBytes)
  expect(removeBin).toHaveBeenCalledWith(replacementSlot)
  expect(removeBin).not.toHaveBeenCalledWith(sharedSlot)
  expect(symlinkDir).toHaveBeenCalledTimes(2)
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
  const publicationError = new Error('hash-link publication failed')
  publicationLinkFailure = publicationError
  restorationLinkFailure = new Error('hash-link restoration failed')

  let thrown: unknown
  try {
    await publishGlobalInstall({
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
  if (!util.types.isNativeError(thrown)) throw new Error('Expected publishGlobalInstall to throw a native error')
  expect(thrown).toMatchObject({
    code: 'ERR_PNPM_GLOBAL_BIN_ROLLBACK_FAILED',
  })
  expect(thrown.cause).toBe(publicationError)
  expect(await readSlotState(replacementSlot)).toStrictEqual(replacementBefore)
  const backupDirs = await findBackupDirs(fixture.root)
  expect(backupDirs).toHaveLength(1)
  expect(thrown.message).toContain(backupDirs[0])
  expect(thrown.message).toContain(fixture.freshInstallDir)
  expect(existsSync(backupDirs[0])).toBe(true)
  expect(existsSync(fixture.freshInstallDir)).toBe(true)
  expect(symlinkDir).toHaveBeenCalledTimes(2)
})

test('preserves Windows file and directory symlink kinds with relative targets', async () => {
  const platform = Object.getOwnPropertyDescriptor(process, 'platform')
  if (platform == null) throw new Error('Expected process.platform to be an own property')
  Object.defineProperty(process, 'platform', { ...platform, value: 'win32' })
  const realSymlink = fs.symlink.bind(fs)
  const backupSymlinkTypes: Array<string | null | undefined> = []
  const symlink = jest.spyOn(fs, 'symlink').mockImplementation(async (target, linkPath, type) => {
    if (String(linkPath).includes(`${path.sep}.pnpm-bin-backup-`)) {
      backupSymlinkTypes.push(type)
    }
    await realSymlink(target, linkPath, type)
  })
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
    await fs.symlink(fileTarget, fileLink, 'file')
    await fs.symlink(dirTarget, dirLink, 'dir')
    const publicationError = new Error('hash-link publication failed')
    publicationLinkFailure = publicationError

    await expect(publishGlobalInstall({
      installDir: fixture.freshInstallDir,
      hashLink: fixture.hashLink,
      globalBinDir: fixture.globalBinDir,
      pkgs: [{ manifest, location: fixture.packageDir }],
      binsToSkip: new Set(),
    })).rejects.toBe(publicationError)

    expect(backupSymlinkTypes).toHaveLength(2)
    expect(new Set(backupSymlinkTypes)).toStrictEqual(new Set(['file', 'dir']))
    expect(await fs.readlink(fileLink)).toBe(fileTarget)
    expect(await fs.readlink(dirLink)).toBe(dirTarget)
    expect((await fs.stat(fileLink)).isFile()).toBe(true)
    expect((await fs.stat(dirLink)).isDirectory()).toBe(true)
  } finally {
    symlink.mockRestore()
    Object.defineProperty(process, 'platform', platform)
  }
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
  const publicationError = new Error('hash-link publication failed')
  const cleanupError = new Error('fresh-install cleanup failed')
  publicationLinkFailure = publicationError
  freshCleanupFailure = { path: fixture.freshInstallDir, error: cleanupError }

  let thrown: unknown
  try {
    await publishGlobalInstall({
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
  expect(thrown.cause).toBe(publicationError)
  expect(thrown.errors).toEqual(expect.arrayContaining([publicationError, cleanupError]))
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
  const publicationError = new Error('hash-link publication failed')
  publicationLinkFailure = publicationError
  obstructBackupCleanup = true

  let thrown: unknown
  try {
    await publishGlobalInstall({
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
  expect(thrown.cause).toBe(publicationError)
  expect(thrown.errors).toContain(publicationError)
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
  const publicationError = new Error('hash-link publication failed')
  const freshCleanupError = new Error('fresh-install cleanup failed')
  publicationLinkFailure = publicationError
  obstructBackupCleanup = true
  freshCleanupFailure = { path: fixture.freshInstallDir, error: freshCleanupError }

  let thrown: unknown
  try {
    await publishGlobalInstall({
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
  expect(thrown.errors).toEqual(expect.arrayContaining([publicationError, freshCleanupError]))
  const backupCleanupErrors = thrown.errors.filter((error) => {
    return error !== publicationError && error !== freshCleanupError
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

  const publishedBins = await publishGlobalInstall({
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
    publishedBins,
    protectedBins: new Set(),
  })

  expect(publishedBins).toStrictEqual(new Set(['tool']))
  await expect(fs.lstat(toolSlot)).rejects.toMatchObject({ code: 'ENOENT' })
  expect(existsSync(fixture.oldInstallDir)).toBe(false)
  expect(await fs.realpath(fixture.hashLink)).toBe(await fs.realpath(fixture.freshInstallDir))
})

test('preserves bin slots owned by the published and surviving groups', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-global-cleanup-'))
  testRoot = root
  const globalDir = path.join(root, 'global')
  const globalBinDir = path.join(root, 'bin')
  const oldInstallDir = path.join(globalDir, 'old-install')
  await Promise.all([
    fs.mkdir(globalBinDir, { recursive: true }),
    fs.mkdir(oldInstallDir, { recursive: true }),
  ])
  const publishedSlot = path.join(globalBinDir, 'published')
  const protectedSlot = path.join(globalBinDir, 'protected')
  await Promise.all([
    fs.writeFile(publishedSlot, 'published\n'),
    fs.writeFile(protectedSlot, 'protected\n'),
  ])
  getInstalledBinNames.mockResolvedValue(['published', 'protected'])

  await cleanupReplacedGlobalInstalls({
    groups: [{ dependencies: { old: '1.0.0' }, hash: 'active-hash', installDir: oldInstallDir }],
    globalDir,
    globalBinDir,
    activeHash: 'active-hash',
    publishedBins: new Set(['published']),
    protectedBins: new Set(['protected']),
  })

  expect(await fs.readFile(publishedSlot, 'utf8')).toBe('published\n')
  expect(await fs.readFile(protectedSlot, 'utf8')).toBe('protected\n')
})

test('removes stale bins and the old install without removing the active hash link', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-global-cleanup-'))
  testRoot = root
  const globalDir = path.join(root, 'global')
  const globalBinDir = path.join(root, 'bin')
  const oldInstallDir = path.join(globalDir, 'old-install')
  const activeInstallDir = path.join(globalDir, 'active-install')
  const hashLink = path.join(globalDir, 'active-hash')
  await Promise.all([
    fs.mkdir(globalBinDir, { recursive: true }),
    fs.mkdir(oldInstallDir, { recursive: true }),
    fs.mkdir(activeInstallDir, { recursive: true }),
  ])
  const staleSlot = path.join(globalBinDir, 'stale')
  await fs.writeFile(staleSlot, 'stale\n')
  await replaceDirectorySymlink(activeInstallDir, hashLink)
  getInstalledBinNames.mockResolvedValue(['stale'])

  await cleanupReplacedGlobalInstalls({
    groups: [{ dependencies: { old: '1.0.0' }, hash: 'active-hash', installDir: oldInstallDir }],
    globalDir,
    globalBinDir,
    activeHash: 'active-hash',
    publishedBins: new Set(),
    protectedBins: new Set(),
  })

  await expect(fs.lstat(staleSlot)).rejects.toMatchObject({ code: 'ENOENT' })
  expect(existsSync(oldInstallDir)).toBe(false)
  expect(await fs.realpath(hashLink)).toBe(await fs.realpath(activeInstallDir))
})

test('does not delete an install directory outside the global directory', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-global-cleanup-'))
  testRoot = root
  const globalDir = path.join(root, 'global')
  const globalBinDir = path.join(root, 'bin')
  const outsideInstallDir = path.join(root, 'outside-install')
  await Promise.all([
    fs.mkdir(globalDir, { recursive: true }),
    fs.mkdir(globalBinDir, { recursive: true }),
    fs.mkdir(outsideInstallDir, { recursive: true }),
  ])
  const marker = path.join(outsideInstallDir, 'marker')
  await fs.writeFile(marker, 'outside\n')
  getInstalledBinNames.mockResolvedValue([])

  await cleanupReplacedGlobalInstalls({
    groups: [{ dependencies: { old: '1.0.0' }, hash: 'active-hash', installDir: outsideInstallDir }],
    globalDir,
    globalBinDir,
    activeHash: 'active-hash',
    publishedBins: new Set(),
    protectedBins: new Set(),
  })

  expect(await fs.readFile(marker, 'utf8')).toBe('outside\n')
})

interface PublicationFixture {
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

async function createFixture (manifest: DependencyManifest): Promise<PublicationFixture> {
  if (typeof manifest.bin !== 'object' || manifest.bin == null) {
    throw new Error('The publication fixture requires an explicit bin map')
  }
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-global-publication-'))
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
