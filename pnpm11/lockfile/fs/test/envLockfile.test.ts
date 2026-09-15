import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { createEnvLockfile, extractMainDocument, readEnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import { temporaryDirectory } from 'tempy'

const testOnNonWindows = process.platform === 'win32' ? test.skip : test

testOnNonWindows('readEnvLockfile reads a symlinked lockfile', async () => {
  const dir = temporaryDirectory()
  const realLockfile = path.join(dir, 'real-lockfile.yaml')
  fs.writeFileSync(realLockfile, '---\nlockfileVersion: "9.0"\nimporters:\n  .:\n    configDependencies: {}\npackages: {}\nsnapshots: {}\n---\n')
  fs.symlinkSync(realLockfile, path.join(dir, WANTED_LOCKFILE), 'file')

  await expect(readEnvLockfile(dir)).resolves.toMatchObject({
    lockfileVersion: '9.0',
  })
})

testOnNonWindows('writeEnvLockfile rejects a symlinked lockfile without touching the target', async () => {
  const dir = temporaryDirectory()
  const realLockfile = path.join(dir, 'real-lockfile.yaml')
  fs.writeFileSync(realLockfile, 'target content')
  const lockfilePath = path.join(dir, WANTED_LOCKFILE)
  fs.symlinkSync(realLockfile, lockfilePath, 'file')

  await expect(writeEnvLockfile(dir, createEnvLockfile())).rejects.toThrow(/symlinked lockfile/)

  expect(fs.lstatSync(lockfilePath).isSymbolicLink()).toBe(true)
  expect(fs.readFileSync(realLockfile, 'utf8')).toBe('target content')
})

testOnNonWindows('writeEnvLockfile rejects a symlink inserted while writing without touching the target', async () => {
  const dir = temporaryDirectory()
  const lockfilePath = path.join(dir, WANTED_LOCKFILE)
  const originalLockfile = path.join(dir, 'original-lockfile.yaml')
  const symlinkTarget = path.join(dir, 'symlink-target.yaml')
  fs.writeFileSync(lockfilePath, "lockfileVersion: '9.0'\n")
  fs.writeFileSync(symlinkTarget, 'target content')
  const open = fs.promises.open
  const openSpy = jest.spyOn(fs.promises, 'open').mockImplementation(async (filePath, flags, mode) => {
    const fileHandle = await open(filePath, flags, mode)
    if (String(filePath).endsWith('.tmp')) {
      fs.renameSync(lockfilePath, originalLockfile)
      fs.symlinkSync(symlinkTarget, lockfilePath, 'file')
    }
    return fileHandle
  })

  try {
    await expect(writeEnvLockfile(dir, createEnvLockfile())).rejects.toThrow(/symlinked lockfile/)
  } finally {
    openSpy.mockRestore()
  }

  expect(fs.lstatSync(lockfilePath).isSymbolicLink()).toBe(true)
  expect(fs.readFileSync(symlinkTarget, 'utf8')).toBe('target content')
  expect(fs.readdirSync(dir).some((name) => name.endsWith('.tmp'))).toBe(false)
})

test('writeEnvLockfile keeps the main document after the env document it replaces', async () => {
  const dir = temporaryDirectory()
  const lockfilePath = path.join(dir, WANTED_LOCKFILE)
  const mainDoc = "lockfileVersion: '9.0'\nsettings:\n  autoInstallPeers: true\n"
  fs.writeFileSync(lockfilePath, `---\nlockfileVersion: '9.0'\nimporters:\n  .:\n    configDependencies: {}\npackages: {}\nsnapshots: {}\n\n---\n${mainDoc}`)

  await writeEnvLockfile(dir, envLockfileWithConfigDep())

  const written = fs.readFileSync(lockfilePath, 'utf8')
  expect(written).toBe(`---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies:
      my-config:
        specifier: ^1.0.0
        version: 1.0.0

packages: {}

snapshots: {}

---
${mainDoc}`)
  await expect(readEnvLockfile(dir)).resolves.toMatchObject({
    importers: { '.': { configDependencies: { 'my-config': { version: '1.0.0' } } } },
  })
})

test('writeEnvLockfile leaves no temporary file behind', async () => {
  const dir = temporaryDirectory()

  await writeEnvLockfile(dir, envLockfileWithConfigDep())

  expect(fs.readdirSync(dir)).toStrictEqual([WANTED_LOCKFILE])
})

testOnNonWindows('writeEnvLockfile accepts a symlinked lockfile when the env document is unchanged', async () => {
  const source = temporaryDirectory()
  const lockfile = envLockfileWithConfigDep()
  await writeEnvLockfile(source, lockfile)
  const content = fs.readFileSync(path.join(source, WANTED_LOCKFILE), 'utf8')

  const dir = temporaryDirectory()
  const realLockfile = path.join(dir, 'real-lockfile.yaml')
  fs.writeFileSync(realLockfile, content)
  const lockfilePath = path.join(dir, WANTED_LOCKFILE)
  fs.symlinkSync(realLockfile, lockfilePath, 'file')

  await expect(writeEnvLockfile(dir, lockfile)).resolves.toBeUndefined()

  expect(fs.lstatSync(lockfilePath).isSymbolicLink()).toBe(true)
  expect(fs.readFileSync(realLockfile, 'utf8')).toBe(content)
})

test('writeEnvLockfile leaves an unchanged CRLF lockfile untouched', async () => {
  const dir = temporaryDirectory()
  const lockfilePath = path.join(dir, WANTED_LOCKFILE)
  const lockfile = envLockfileWithConfigDep()
  await writeEnvLockfile(dir, lockfile)
  const crlfContent = fs.readFileSync(lockfilePath, 'utf8').replace(/\n/g, '\r\n')
  fs.writeFileSync(lockfilePath, crlfContent)
  const mtimeBefore = fs.statSync(lockfilePath).mtimeMs

  await writeEnvLockfile(dir, lockfile)

  expect(fs.readFileSync(lockfilePath, 'utf8')).toBe(crlfContent)
  expect(fs.statSync(lockfilePath).mtimeMs).toBe(mtimeBefore)
})

test('writeEnvLockfile replaces the env document of a lockfile that carries a BOM', async () => {
  const dir = temporaryDirectory()
  const lockfilePath = path.join(dir, WANTED_LOCKFILE)
  const mainDoc = "lockfileVersion: '9.0'\nsettings:\n  autoInstallPeers: true\n"
  const oldEnvDoc = "lockfileVersion: '9.0'\nimporters:\n  .:\n    configDependencies: {}\npackages: {}\nsnapshots: {}\n"
  fs.writeFileSync(lockfilePath, `\uFEFF---\n${oldEnvDoc}\n---\n${mainDoc}`)

  await writeEnvLockfile(dir, envLockfileWithConfigDep())

  const written = fs.readFileSync(lockfilePath, 'utf8')
  expect(extractMainDocument(written)).toBe(mainDoc)
  expect(written.startsWith('---\n')).toBe(true)
  await expect(readEnvLockfile(dir)).resolves.toMatchObject({
    importers: { '.': { configDependencies: { 'my-config': { specifier: '^1.0.0', version: '1.0.0' } } } },
  })
})

testOnNonWindows('writeEnvLockfile preserves the lockfile mode against the umask', async () => {
  const dir = temporaryDirectory()
  const lockfilePath = path.join(dir, WANTED_LOCKFILE)
  await writeEnvLockfile(dir, createEnvLockfile())
  fs.chmodSync(lockfilePath, 0o666)
  const previousUmask = process.umask(0o022)
  try {
    await writeEnvLockfile(dir, envLockfileWithConfigDep())
  } finally {
    process.umask(previousUmask)
  }

  expect(fs.statSync(lockfilePath).mode & 0o777).toBe(0o666)
})

function envLockfileWithConfigDep () {
  const lockfile = createEnvLockfile()
  lockfile.importers['.'].configDependencies = {
    'my-config': { specifier: '^1.0.0', version: '1.0.0' },
  }
  return lockfile
}
