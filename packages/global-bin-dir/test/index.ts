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
const otherDir = makePath('some', 'dir')
const currentExecDir = makePath('current', 'exec')
process.env[FAKE_PATH] = [
  userGlobalBin,
  nodeGlobalBin,
  npmGlobalBin,
  otherDir,
  currentExecDir,
].join(path.delimiter)

test('prefer a directory that has "nodejs" in the path', (t) => {
  canWriteToDir = () => true
  t.equal(globalBinDir(), nodeGlobalBin)
  t.end()
})

test('prefer the first directory that has "nodejs" or "npm" in the path and to which the process has write access', (t) => {
  canWriteToDir = (dir) => dir !== nodeGlobalBin
  t.equal(globalBinDir(), npmGlobalBin)
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
  canWriteToDir = (dir) => dir !== nodeGlobalBin && dir !== npmGlobalBin
  t.equal(globalBinDir(), currentExecDir)
  process.execPath = originalExecPath
  t.end()
})

test('when the process has write access only to one of the directories, return it', (t) => {
  canWriteToDir = (dir) => dir === otherDir
  t.equal(globalBinDir(), otherDir)
  t.end()
})

test('throw exception if PATH is not set', (t) => {
  const pathEnv = process.env[FAKE_PATH]
  delete process.env[FAKE_PATH]
  t.throws(() => globalBinDir(), /Couldn't find a global directory/)
  process.env[FAKE_PATH] = pathEnv
  t.end()
})
