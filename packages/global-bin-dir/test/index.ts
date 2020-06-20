import PnpmError from '@pnpm/error'
import { sync as _canWriteToDir } from 'can-write-to-dir'
import isWindows = require('is-windows')
import path = require('path')
import proxiquire = require('proxyquire')
import test = require('tape')

const makePath =
  isWindows()
    ? (...paths: string[]) => `C:\\${path.join(...paths)}`
    : (...paths: string[]) => `/${path.join(...paths)}`

let canWriteToDir!: typeof _canWriteToDir
const FAKE_PATH = 'FAKE_PATH'

const globalBinDir = proxiquire('../lib/index.js', {
  'can-write-to-dir': {
    sync: (dir: string) => canWriteToDir(dir),
  },
  'path-name': FAKE_PATH,
}).default

const userGlobalBin = makePath('usr', 'local', 'bin')
const nodeGlobalBin = makePath('home', 'z', '.nvs', 'node', '12.0.0', 'x64', 'bin')
const npmGlobalBin = makePath('home', 'z', '.npm')
const pnpmGlobalBin = makePath('home', 'z', '.pnpm')
const otherDir = makePath('some', 'dir')
const currentExecDir = makePath('current', 'exec')
process.env[FAKE_PATH] = [
  userGlobalBin,
  nodeGlobalBin,
  npmGlobalBin,
  pnpmGlobalBin,
  otherDir,
  currentExecDir,
].join(path.delimiter)

test('prefer a directory that has "nodejs", "npm", or "pnpm" in the path', (t) => {
  canWriteToDir = () => true
  t.equal(globalBinDir(), nodeGlobalBin)

  canWriteToDir = (dir) => dir !== nodeGlobalBin
  t.equal(globalBinDir(), npmGlobalBin)

  canWriteToDir = (dir) => dir !== nodeGlobalBin && dir !== npmGlobalBin
  t.equal(globalBinDir(), pnpmGlobalBin)

  t.end()
})

test("ignore directories that don't exist", (t) => {
  canWriteToDir = (dir) => {
    if (dir === nodeGlobalBin) {
      const err = new Error('Not exists')
      err['code'] = 'ENOENT'
      throw err
    }
    return true
  }
  t.equal(globalBinDir(), npmGlobalBin)
  t.end()
})

test('prefer the directory of the currently executed nodejs command', (t) => {
  const originalExecPath = process.execPath
  process.execPath = path.join(currentExecDir, 'n')
  canWriteToDir = (dir) => dir !== nodeGlobalBin && dir !== npmGlobalBin && dir !== pnpmGlobalBin
  t.equal(globalBinDir(), currentExecDir)
  process.execPath = originalExecPath
  t.end()
})

test('when the process has no write access to any of the suitable directories, throw an error', (t) => {
  canWriteToDir = (dir) => dir === otherDir
  let err!: PnpmError
  try {
    globalBinDir()
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_GLOBAL_BIN_DIR_PERMISSION')
  t.end()
})

test('throw an exception if non of the directories in the PATH are suitable', (t) => {
  const pathEnv = process.env[FAKE_PATH]
  process.env[FAKE_PATH] = [otherDir].join(path.delimiter)
  canWriteToDir = () => true
  let err!: PnpmError
  try {
    globalBinDir()
  } catch (_err) {
    err = _err
  }
  t.ok(err)
  t.equal(err.code, 'ERR_PNPM_NO_GLOBAL_BIN_DIR')
  process.env[FAKE_PATH] = pathEnv
  t.end()
})

test('throw exception if PATH is not set', (t) => {
  const pathEnv = process.env[FAKE_PATH]
  delete process.env[FAKE_PATH]
  t.throws(() => globalBinDir(), /Couldn't find a global directory/)
  process.env[FAKE_PATH] = pathEnv
  t.end()
})
