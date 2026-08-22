import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'

const removeBin = jest.fn<(cmd: string) => Promise<void>>().mockResolvedValue(undefined)

jest.unstable_mockModule('@pnpm/bins.remover', () => ({ removeBin }))

const { handleGlobalRemove } = await import('../src/globalRemove.js')

beforeEach(() => {
  removeBin.mockClear()
})

// A malicious global package whose manifest declares reserved bin keys must not
// reach the deletion sink: `path.join(globalBinDir, '.')` is the bin directory
// itself and `path.join(globalBinDir, '..')` is its parent, so removing either
// would wipe out unrelated files. Only the safe `good` shim may be deleted.
test('global remove ignores reserved manifest bin names', async () => {
  const globalDir = fs.mkdtempSync(path.join(os.tmpdir(), 'global-remove-'))
  const globalBinDir = path.join(globalDir, 'bin')
  const installDir = path.join(globalDir, 'install')
  const depDir = path.join(installDir, 'node_modules', 'evil')
  fs.mkdirSync(depDir, { recursive: true })
  fs.writeFileSync(
    path.join(installDir, 'package.json'),
    JSON.stringify({ name: 'global', version: '1.0.0', dependencies: { evil: '1.0.0' } })
  )
  fs.writeFileSync(
    path.join(depDir, 'package.json'),
    JSON.stringify({
      name: 'evil',
      version: '1.0.0',
      bin: {
        '': './empty.js',
        '.': './dot.js',
        '..': './dot-dot.js',
        '@scope/..': './scoped-dot-dot.js',
        good: './good.js',
      },
    })
  )
  fs.symlinkSync(installDir, path.join(globalDir, 'hash'))

  await handleGlobalRemove({ globalPkgDir: globalDir, bin: globalBinDir }, ['evil'])

  expect(removeBin).toHaveBeenCalledTimes(1)
  expect(removeBin).toHaveBeenCalledWith(path.join(globalBinDir, 'good'))
})

test('global remove checks every target before deleting any group', async () => {
  const globalDir = fs.mkdtempSync(path.join(os.tmpdir(), 'global-remove-target-preflight-'))
  assertTemporaryRoot(globalDir)
  const globalBinDir = path.join(globalDir, 'bin')
  fs.mkdirSync(globalBinDir, { recursive: true })
  const readable = createGlobalGroup(globalDir, 'readable-hash', 'readable', {
    name: 'readable',
    version: '1.0.0',
    bin: { readable: 'bin/readable.js' },
  })
  const incomplete = createGlobalGroup(globalDir, 'incomplete-hash', 'incomplete')
  const readableSlot = path.join(globalBinDir, 'readable')
  fs.writeFileSync(readableSlot, 'readable shim\n')
  const before = snapshotFilesystem(globalDir)
  const assertFailedAttempt = async (attempt: number): Promise<void> => {
    const failure = await captureError(() => handleGlobalRemove(
      { globalPkgDir: globalDir, bin: globalBinDir },
      ['readable', 'incomplete']
    ))
    expect({ attempt, errorCode: getErrorCode(failure) }).toStrictEqual({ attempt, errorCode: 'ENOENT' })
    expect(snapshotFilesystem(globalDir)).toStrictEqual(before)
    expect(removeBin).not.toHaveBeenCalled()
  }

  try {
    await assertFailedAttempt(1)
    await assertFailedAttempt(2)

    writeDependencyManifest(incomplete, {
      name: 'incomplete',
      version: '1.0.0',
    })
    await handleGlobalRemove({ globalPkgDir: globalDir, bin: globalBinDir }, ['readable', 'incomplete'])

    expect(removeBin).toHaveBeenCalledTimes(1)
    expect(removeBin).toHaveBeenCalledWith(readableSlot)
    expect(fs.existsSync(readable.hashLink)).toBe(false)
    expect(fs.existsSync(readable.installDir)).toBe(false)
    expect(fs.existsSync(incomplete.hashLink)).toBe(false)
    expect(fs.existsSync(incomplete.installDir)).toBe(false)

    const afterSuccess = snapshotFilesystem(globalDir)
    const repeatError = await captureError(() => handleGlobalRemove(
      { globalPkgDir: globalDir, bin: globalBinDir },
      ['readable', 'incomplete']
    ))
    expect(getErrorCode(repeatError)).toBe('ERR_PNPM_GLOBAL_PKG_NOT_FOUND')
    expect(snapshotFilesystem(globalDir)).toStrictEqual(afterSuccess)
    expect(removeBin).toHaveBeenCalledTimes(1)
  } finally {
    fs.rmSync(globalDir, { recursive: true, force: true })
  }
})

test('global remove checks surviving ownership before deleting a target', async () => {
  const globalDir = fs.mkdtempSync(path.join(os.tmpdir(), 'global-remove-survivor-preflight-'))
  assertTemporaryRoot(globalDir)
  const globalBinDir = path.join(globalDir, 'bin')
  fs.mkdirSync(globalBinDir, { recursive: true })
  const target = createGlobalGroup(globalDir, 'target-hash', 'target', {
    name: 'target',
    version: '1.0.0',
    bin: { shared: 'bin/shared.js' },
  })
  const survivor = createGlobalGroup(globalDir, 'survivor-hash', 'survivor')
  const sharedSlot = path.join(globalBinDir, 'shared')
  fs.writeFileSync(sharedSlot, 'shared shim\n')
  const before = snapshotFilesystem(globalDir)
  const assertFailedAttempt = async (attempt: number): Promise<void> => {
    const failure = await captureError(() => handleGlobalRemove(
      { globalPkgDir: globalDir, bin: globalBinDir },
      ['target']
    ))
    expect({ attempt, errorCode: getErrorCode(failure) }).toStrictEqual({ attempt, errorCode: 'ENOENT' })
    expect(snapshotFilesystem(globalDir)).toStrictEqual(before)
    expect(removeBin).not.toHaveBeenCalled()
  }

  try {
    await assertFailedAttempt(1)
    await assertFailedAttempt(2)

    writeDependencyManifest(survivor, {
      name: 'survivor',
      version: '1.0.0',
      bin: { shared: 'bin/shared.js' },
    })
    await handleGlobalRemove({ globalPkgDir: globalDir, bin: globalBinDir }, ['target'])

    expect(removeBin).not.toHaveBeenCalled()
    expect(fs.existsSync(target.hashLink)).toBe(false)
    expect(fs.existsSync(target.installDir)).toBe(false)
    expect(fs.existsSync(survivor.hashLink)).toBe(true)
    expect(fs.realpathSync(survivor.hashLink)).toBe(fs.realpathSync(survivor.installDir))
    expect(fs.readFileSync(survivor.marker, 'utf8')).toBe('survivor install\n')
    expect(fs.readFileSync(sharedSlot, 'utf8')).toBe('shared shim\n')

    const afterSuccess = snapshotFilesystem(globalDir)
    const repeatError = await captureError(() => handleGlobalRemove(
      { globalPkgDir: globalDir, bin: globalBinDir },
      ['target']
    ))
    expect(getErrorCode(repeatError)).toBe('ERR_PNPM_GLOBAL_PKG_NOT_FOUND')
    expect(snapshotFilesystem(globalDir)).toStrictEqual(afterSuccess)
    expect(removeBin).not.toHaveBeenCalled()
  } finally {
    fs.rmSync(globalDir, { recursive: true, force: true })
  }
})

interface GlobalGroupFixture {
  dependencyManifestPath: string
  hashLink: string
  installDir: string
  marker: string
}

function createGlobalGroup (
  globalDir: string,
  hash: string,
  alias: string,
  dependencyManifest?: Record<string, unknown>
): GlobalGroupFixture {
  const installDir = path.join(globalDir, `${hash}-install`)
  const depDir = path.join(installDir, 'node_modules', alias)
  const dependencyManifestPath = path.join(depDir, 'package.json')
  const hashLink = path.join(globalDir, hash)
  const marker = path.join(installDir, 'marker')
  assertPathInside(globalDir, installDir)
  fs.mkdirSync(depDir, { recursive: true })
  fs.writeFileSync(marker, `${alias} install\n`)
  fs.writeFileSync(path.join(installDir, 'package.json'), JSON.stringify({
    name: `${alias}-global-group`,
    version: '1.0.0',
    dependencies: { [alias]: '1.0.0' },
  }))
  if (dependencyManifest != null) {
    fs.writeFileSync(dependencyManifestPath, JSON.stringify(dependencyManifest))
  }
  fs.symlinkSync(installDir, hashLink, process.platform === 'win32' ? 'junction' : 'dir')
  return { dependencyManifestPath, hashLink, installDir, marker }
}

function writeDependencyManifest (fixture: GlobalGroupFixture, manifest: Record<string, unknown>): void {
  fs.writeFileSync(fixture.dependencyManifestPath, JSON.stringify(manifest))
}

async function captureError (run: () => Promise<void>): Promise<unknown> {
  try {
    await run()
    return undefined
  } catch (err) {
    return err
  }
}

function getErrorCode (err: unknown): unknown {
  return err != null && typeof err === 'object' && 'code' in err ? err.code : undefined
}

interface FilesystemEntry {
  content?: string
  kind: 'directory' | 'file' | 'symlink'
  path: string
  target?: string
}

function snapshotFilesystem (root: string): FilesystemEntry[] {
  const result: FilesystemEntry[] = []
  visit(root, '')
  return result

  function visit (dir: string, relativeDir: string): void {
    for (const name of fs.readdirSync(dir).sort()) {
      const absolutePath = path.join(dir, name)
      const relativePath = path.join(relativeDir, name)
      const stat = fs.lstatSync(absolutePath)
      if (stat.isSymbolicLink()) {
        result.push({ kind: 'symlink', path: relativePath, target: fs.readlinkSync(absolutePath) })
      } else if (stat.isDirectory()) {
        result.push({ kind: 'directory', path: relativePath })
        visit(absolutePath, relativePath)
      } else {
        result.push({ content: fs.readFileSync(absolutePath).toString('base64'), kind: 'file', path: relativePath })
      }
    }
  }
}

function assertTemporaryRoot (root: string): void {
  expect(fs.realpathSync(path.dirname(root))).toBe(fs.realpathSync(os.tmpdir()))
}

function assertPathInside (root: string, candidate: string): void {
  const relative = path.relative(root, candidate)
  expect(path.isAbsolute(relative) || relative === '..' || relative.startsWith(`..${path.sep}`)).toBe(false)
}
