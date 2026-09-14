import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'

const execaSync = jest.fn()
jest.unstable_mockModule('execa', () => ({
  sync: execaSync,
}))

const detectIfCurrentPkgIsExecutable = jest.fn<() => boolean>(() => false)
jest.unstable_mockModule('@pnpm/cli.meta', () => ({
  detectIfCurrentPkgIsExecutable,
}))

const { runPnpmCli } = await import('@pnpm/exec.pnpm-cli-runner')

const originalArgv = [...process.argv]
const tempDirs: string[] = []

afterEach(() => {
  execaSync.mockClear()
  detectIfCurrentPkgIsExecutable.mockReturnValue(false)
  process.argv = [...originalArgv]
  while (tempDirs.length > 0) {
    fs.rmSync(tempDirs.pop()!, { recursive: true, force: true })
  }
})

/**
 * An installed package whose bin is reached through a `node_modules/.bin` link,
 * the shape npm creates. Returns the link, which is what `process.argv[1]`
 * would be.
 */
function installPackage (pkgName: string, binName: string, scriptName: string): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-cli-runner-'))
  tempDirs.push(root)
  const pkgDir = path.join(root, 'node_modules', pkgName)
  const binDir = path.join(root, 'node_modules', '.bin')
  fs.mkdirSync(path.join(pkgDir, 'bin'), { recursive: true })
  fs.mkdirSync(binDir, { recursive: true })
  fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ name: 'consuming-project' }))
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: pkgName }))
  const script = path.join(pkgDir, 'bin', scriptName)
  fs.writeFileSync(script, '')
  const link = path.join(binDir, binName)
  fs.symlinkSync(script, link)
  return link
}

test('the @pnpm/exe build re-runs itself', () => {
  detectIfCurrentPkgIsExecutable.mockReturnValue(true)

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, ['add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test.each([
  ['pnpm', 'pnpm.mjs'],
  ['pn', 'pnpm.mjs'],
  ['pnpm', 'pnpm.cjs'],
])('pnpm reached through node_modules/.bin/%s is re-run with Node.js', (binName, scriptName) => {
  const entryScript = installPackage('pnpm', binName, scriptName)
  process.argv[1] = entryScript

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, [entryScript, 'add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('an entry script with no package manifest above it is still re-run', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-cli-runner-'))
  tempDirs.push(root)
  const entryScript = path.join(root, 'pnpm.mjs')
  fs.writeFileSync(entryScript, '')
  process.argv[1] = entryScript

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, [entryScript, 'add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('an entry script whose link target is gone is still re-run', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-cli-runner-'))
  tempDirs.push(root)
  const entryScript = path.join(root, 'pnpm.mjs')
  fs.symlinkSync(path.join(root, 'missing.mjs'), entryScript)
  process.argv[1] = entryScript

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, [entryScript, 'add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('an entry script under an unreadable manifest is still re-run', () => {
  const entryScript = installPackage('pnpm', 'pnpm', 'pnpm.mjs')
  fs.writeFileSync(path.join(path.dirname(path.dirname(fs.realpathSync(entryScript))), 'package.json'), '{ not json')
  process.argv[1] = entryScript

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, [entryScript, 'add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('the reporter is passed to the spawned CLI', () => {
  process.argv[1] = installPackage('pnpm', 'pnpm', 'pnpm.mjs')

  runPnpmCli(['install'], { cwd: '/test', reporter: 'silent' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, [process.argv[1], 'install', '--reporter=silent'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('another package that publishes a pnpm-like bin falls back to the pnpm on PATH', () => {
  process.argv[1] = installPackage('not-pnpm', 'pn', 'pn')

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith('pnpm', ['add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test.each([
  // A host that merely imports pnpm's packages, such as the Jest runs of the
  // commands that call runPnpmCli.
  '/path/to/node_modules/jest/bin/jest.js',
  // Re-running pnpx would prepend `dlx` to the command.
  '/path/to/pnpm/bin/pnpx.mjs',
])('the entry script %s falls back to the pnpm on PATH', (entryScript) => {
  process.argv[1] = entryScript

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith('pnpm', ['add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('a process without an entry script falls back to the pnpm on PATH', () => {
  process.argv = [process.argv[0]]

  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith('pnpm', ['add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})
