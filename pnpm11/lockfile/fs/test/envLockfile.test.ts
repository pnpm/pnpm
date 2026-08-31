import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
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

test('writeEnvLockfile keeps the main document after the env document it replaces', async () => {
  const dir = temporaryDirectory()
  const lockfilePath = path.join(dir, WANTED_LOCKFILE)
  const mainDoc = "lockfileVersion: '9.0'\nsettings:\n  autoInstallPeers: true\n"
  fs.writeFileSync(lockfilePath, `---\nlockfileVersion: '9.0'\nimporters:\n  .:\n    configDependencies: {}\npackages: {}\nsnapshots: {}\n\n---\n${mainDoc}`)

  await writeEnvLockfile(dir, envLockfileWithConfigDep())

  const written = fs.readFileSync(lockfilePath, 'utf8')
  expect(extractMainDocument(written)).toBe(mainDoc)
  expect(written).toContain('my-config')
  await expect(readEnvLockfile(dir)).resolves.toMatchObject({
    importers: { '.': { configDependencies: { 'my-config': { version: '1.0.0' } } } },
  })
})

test('writeEnvLockfile leaves no temporary file behind', async () => {
  const dir = temporaryDirectory()

  await writeEnvLockfile(dir, envLockfileWithConfigDep())

  expect(fs.readdirSync(dir)).toStrictEqual([WANTED_LOCKFILE])
})

// The replacement is a fresh file, so its mode has to be restored explicitly:
// creating it honours the umask, which would strip bits the lockfile carried.
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
