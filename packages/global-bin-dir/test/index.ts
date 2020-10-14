import globalBinDir from '../src/index'

import PnpmError from '@pnpm/error'
import { sync as _canWriteToDir } from 'can-write-to-dir'

import path = require('path')
import isWindows = require('is-windows')

const makePath =
  isWindows()
    ? (...paths: string[]) => `C:\\${path.join(...paths)}`
    : (...paths: string[]) => `/${path.join(...paths)}`

let canWriteToDir!: typeof _canWriteToDir
let readdirSync = (dir: string) => [] as Array<{ name: string, isDirectory: () => boolean }>
const FAKE_PATH = 'FAKE_PATH'

function makeFileEntry (name: string) {
  return { name, isDirectory: () => false }
}

function makeDirEntry (name: string) {
  return { name, isDirectory: () => true }
}

jest.mock('can-write-to-dir', () => ({
  sync: (dir: string) => canWriteToDir(dir),
}))

jest.mock('fs', () => {
  const originalModule = jest.requireActual('fs')
  return {
    ...originalModule,
    readdirSync: (dir: string) => readdirSync(dir),
  }
})

jest.mock('path-name', () => 'FAKE_PATH')

const userGlobalBin = makePath('usr', 'local', 'bin')
const nodeGlobalBin = makePath('home', 'z', '.nvs', 'node', '12.0.0', 'x64', 'bin')
const npmGlobalBin = makePath('home', 'z', '.npm')
const pnpmGlobalBin = makePath('home', 'z', '.pnpm')
const otherDir = makePath('some', 'dir')
const currentExecDir = makePath('current', 'exec')
const dirWithTrailingSlash = `${makePath('current', 'slash')}${path.sep}`
process.env[FAKE_PATH] = [
  userGlobalBin,
  nodeGlobalBin,
  npmGlobalBin,
  pnpmGlobalBin,
  otherDir,
  currentExecDir,
  dirWithTrailingSlash,
].join(path.delimiter)

test('prefer a directory that has "nodejs", "npm", or "pnpm" in the path', () => {
  canWriteToDir = () => true
  expect(globalBinDir()).toStrictEqual(nodeGlobalBin)

  canWriteToDir = (dir) => dir !== nodeGlobalBin
  expect(globalBinDir()).toStrictEqual(npmGlobalBin)

  canWriteToDir = (dir) => dir !== nodeGlobalBin && dir !== npmGlobalBin
  expect(globalBinDir()).toStrictEqual(pnpmGlobalBin)
})

test('prefer directory that is passed in as a known suitable location', () => {
  canWriteToDir = () => true
  expect(globalBinDir([userGlobalBin])).toStrictEqual(userGlobalBin)
})

test("ignore directories that don't exist", () => {
  canWriteToDir = (dir) => {
    if (dir === nodeGlobalBin) {
      const err = new Error('Not exists')
      err['code'] = 'ENOENT'
      throw err
    }
    return true
  }
  expect(globalBinDir()).toEqual(npmGlobalBin)
})

test('prefer the directory of the currently executed nodejs command', () => {
  const originalExecPath = process.execPath
  process.execPath = path.join(currentExecDir, 'n')
  canWriteToDir = (dir) => dir !== nodeGlobalBin && dir !== npmGlobalBin && dir !== pnpmGlobalBin
  expect(globalBinDir()).toEqual(currentExecDir)

  process.execPath = path.join(dirWithTrailingSlash, 'n')
  expect(globalBinDir()).toEqual(dirWithTrailingSlash)

  process.execPath = originalExecPath
})

test('when the process has no write access to any of the suitable directories, throw an error', () => {
  canWriteToDir = (dir) => dir === otherDir
  let err!: PnpmError
  try {
    globalBinDir()
  } catch (_err) {
    err = _err
  }
  expect(err).toBeDefined()
  expect(err.code).toEqual('ERR_PNPM_GLOBAL_BIN_DIR_PERMISSION')
})

test('when the process has no write access to any of the suitable directories, but opts.shouldAllowWrite is false, return the first match', () => {
  canWriteToDir = (dir) => dir === otherDir
  expect(globalBinDir([], { shouldAllowWrite: false })).toEqual(nodeGlobalBin)
})

test('throw an exception if non of the directories in the PATH are suitable', () => {
  const pathEnv = process.env[FAKE_PATH]
  process.env[FAKE_PATH] = [otherDir].join(path.delimiter)
  canWriteToDir = () => true
  let err!: PnpmError
  try {
    globalBinDir()
  } catch (_err) {
    err = _err
  }
  expect(err).toBeDefined()
  expect(err.code).toEqual('ERR_PNPM_NO_GLOBAL_BIN_DIR')
  process.env[FAKE_PATH] = pathEnv
})

test('throw exception if PATH is not set', () => {
  const pathEnv = process.env[FAKE_PATH]
  delete process.env[FAKE_PATH]
  expect(() => globalBinDir()).toThrow(/Couldn't find a global directory/)
  process.env[FAKE_PATH] = pathEnv
})

test('prefer a directory that has "Node" in the path', () => {
  const capitalizedNodeGlobalBin = makePath('home', 'z', '.nvs', 'Node', '12.0.0', 'x64', 'bin')
  const pathEnv = process.env[FAKE_PATH]
  process.env[FAKE_PATH] = capitalizedNodeGlobalBin

  canWriteToDir = () => true
  expect(globalBinDir()).toEqual(capitalizedNodeGlobalBin)

  process.env[FAKE_PATH] = pathEnv
})

test('select a directory that has a node command in it', () => {
  const dir1 = makePath('foo')
  const dir2 = makePath('bar')
  const pathEnv = process.env[FAKE_PATH]
  process.env[FAKE_PATH] = [
    dir1,
    dir2,
  ].join(path.delimiter)

  canWriteToDir = () => true
  readdirSync = (dir) => dir === dir2 ? [makeFileEntry('node')] : []
  expect(globalBinDir()).toEqual(dir2)

  process.env[FAKE_PATH] = pathEnv
})

test('do not select a directory that has a node directory in it', () => {
  const dir1 = makePath('foo')
  const dir2 = makePath('bar')
  const pathEnv = process.env[FAKE_PATH]
  process.env[FAKE_PATH] = [
    dir1,
    dir2,
  ].join(path.delimiter)

  canWriteToDir = () => true
  readdirSync = (dir) => dir === dir2 ? [makeDirEntry('node')] : []

  expect(() => globalBinDir()).toThrow(/Couldn't find a suitable/)

  process.env[FAKE_PATH] = pathEnv
})

test('select a directory that has a node.bat command in it', () => {
  const dir1 = makePath('foo')
  const dir2 = makePath('bar')
  const pathEnv = process.env[FAKE_PATH]
  process.env[FAKE_PATH] = [
    dir1,
    dir2,
  ].join(path.delimiter)

  canWriteToDir = () => true
  readdirSync = (dir) => dir === dir2 ? [makeFileEntry('node.bat')] : []
  expect(globalBinDir()).toEqual(dir2)

  process.env[FAKE_PATH] = pathEnv
})
