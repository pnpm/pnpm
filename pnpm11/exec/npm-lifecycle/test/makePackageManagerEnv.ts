import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'

import { makePackageManagerEnv } from '../src/makePackageManagerEnv.js'

const originalPath = process.env.PATH
const tempDirs: string[] = []

afterEach(() => {
  process.env.PATH = originalPath
  while (tempDirs.length > 0) {
    fs.rmSync(tempDirs.pop()!, { recursive: true, force: true })
  }
})

// Jest, not pnpm, is the entry here, so npm_execpath falls back to the pnpm on
// PATH. A script's PATH starts with node_modules/.bin, which must not decide it.
test('npm_execpath outside pnpm is the pnpm on this process\'s PATH, not on the script\'s', () => {
  const globalBin = makeBinDirWithPnpm()
  const projectBin = makeBinDirWithPnpm()
  process.env.PATH = globalBin

  const env = makePackageManagerEnv({ PATH: [projectBin, globalBin].join(path.delimiter) })

  expect(path.dirname(env.npm_execpath)).toBe(globalBin)
})

test('npm_execpath outside pnpm is a bare pnpm when no pnpm is on PATH', () => {
  process.env.PATH = makeTempDir()

  expect(makePackageManagerEnv({}).npm_execpath).toBe('pnpm')
})

function makeBinDirWithPnpm (): string {
  const dir = makeTempDir()
  const pnpm = path.join(dir, process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm')
  fs.writeFileSync(pnpm, '', { mode: 0o755 })
  return dir
}

function makeTempDir (): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'npm-lifecycle-'))
  tempDirs.push(dir)
  return dir
}
