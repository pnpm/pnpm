import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { createEnvLockfile, readEnvLockfile, writeEnvLockfile } from '@pnpm/lockfile.fs'
import { temporaryDirectory } from 'tempy'

const testOnNonWindows = process.platform === 'win32' ? test.skip : test

testOnNonWindows('readEnvLockfile rejects a symlinked lockfile', async () => {
  const dir = temporaryDirectory()
  const realLockfile = path.join(dir, 'real-lockfile.yaml')
  fs.writeFileSync(realLockfile, '---\nlockfileVersion: "9.0"\nimporters:\n  .:\n    configDependencies: {}\npackages: {}\nsnapshots: {}\n---\n')
  fs.symlinkSync(realLockfile, path.join(dir, WANTED_LOCKFILE), 'file')

  await expect(readEnvLockfile(dir)).rejects.toThrow(/symlinked lockfile/)
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
