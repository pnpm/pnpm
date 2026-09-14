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
  const entryScript = installPackage({ pkgName: 'pnpm', binName, scriptName })
  process.argv[1] = entryScript

  expectReinvoked(entryScript)
})

test('a pnpm bundle copied into another project is re-run', () => {
  const root = makeTempDir()
  fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ name: 'consuming-project' }))
  fs.mkdirSync(path.join(root, 'tools'))
  const entryScript = path.join(root, 'tools', 'pnpm.mjs')
  fs.writeFileSync(entryScript, '')
  process.argv[1] = entryScript

  expectReinvoked(entryScript)
})

test('an entry script with no package manifest above it is re-run', () => {
  const entryScript = path.join(makeTempDir(), 'pnpm.mjs')
  fs.writeFileSync(entryScript, '')
  process.argv[1] = entryScript

  expectReinvoked(entryScript)
})

test('an entry script whose link target is gone is re-run', () => {
  const root = makeTempDir()
  const entryScript = path.join(root, 'pnpm.mjs')
  fs.symlinkSync(path.join(root, 'missing.mjs'), entryScript)
  process.argv[1] = entryScript

  expectReinvoked(entryScript)
})

test('an entry script under an unreadable manifest is re-run', () => {
  const entryScript = installPackage({ pkgName: 'pnpm', binName: 'pnpm', scriptName: 'pnpm.mjs' })
  const pkgDir = path.dirname(path.dirname(fs.realpathSync(entryScript)))
  fs.writeFileSync(path.join(pkgDir, 'package.json'), '{ not json')
  process.argv[1] = entryScript

  expectReinvoked(entryScript)
})

test('the reporter is passed to the spawned CLI', () => {
  process.argv[1] = installPackage({ pkgName: 'pnpm', binName: 'pnpm', scriptName: 'pnpm.mjs' })

  runPnpmCli(['install'], { cwd: '/test', reporter: 'silent' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, [process.argv[1], 'install', '--reporter=silent'], {
    cwd: '/test',
    stdio: 'inherit',
  })
})

test('a bin that another package claims as its own falls back to the pnpm on PATH', () => {
  process.argv[1] = installPackage({ pkgName: 'not-pnpm', binName: 'pn', scriptName: 'pn' })

  expectFellBackToPath()
})

test('a package whose only bin is named after itself falls back to the pnpm on PATH', () => {
  const root = makeTempDir()
  const pkgDir = path.join(root, 'node_modules', 'pn')
  const binDir = path.join(root, 'node_modules', '.bin')
  fs.mkdirSync(pkgDir, { recursive: true })
  fs.mkdirSync(binDir, { recursive: true })
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({ name: 'pn', bin: './cli.js' }))
  fs.writeFileSync(path.join(pkgDir, 'cli.js'), '')
  fs.symlinkSync(path.join(pkgDir, 'cli.js'), path.join(binDir, 'pn'))
  process.argv[1] = path.join(binDir, 'pn')

  expectFellBackToPath()
})

test.each([
  // A host that merely imports pnpm's packages, such as the Jest runs of the
  // commands that call runPnpmCli.
  '/path/to/node_modules/jest/bin/jest.js',
  // Re-running pnpx would prepend `dlx` to the command.
  '/path/to/pnpm/bin/pnpx.mjs',
])('the entry script %s falls back to the pnpm on PATH', (entryScript) => {
  process.argv[1] = entryScript

  expectFellBackToPath()
})

test('a process without an entry script falls back to the pnpm on PATH', () => {
  process.argv = [process.argv[0]]

  expectFellBackToPath()
})

function expectReinvoked (entryScript: string): void {
  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith(process.execPath, [entryScript, 'add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
}

function expectFellBackToPath (): void {
  runPnpmCli(['add', 'express'], { cwd: '/test' })

  expect(execaSync).toHaveBeenCalledWith('pnpm', ['add', 'express'], {
    cwd: '/test',
    stdio: 'inherit',
  })
}

/** Returns the `.bin` link, which is what `process.argv[1]` would be. */
function installPackage ({ pkgName, binName, scriptName }: { pkgName: string, binName: string, scriptName: string }): string {
  const root = makeTempDir()
  const pkgDir = path.join(root, 'node_modules', pkgName)
  const binDir = path.join(root, 'node_modules', '.bin')
  const script = path.join(pkgDir, 'bin', scriptName)
  fs.mkdirSync(path.join(pkgDir, 'bin'), { recursive: true })
  fs.mkdirSync(binDir, { recursive: true })
  fs.writeFileSync(path.join(root, 'package.json'), JSON.stringify({ name: 'consuming-project' }))
  fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({
    name: pkgName,
    bin: { [binName]: path.posix.join('bin', scriptName) },
  }))
  fs.writeFileSync(script, '')
  const link = path.join(binDir, binName)
  fs.symlinkSync(script, link)
  return link
}

function makeTempDir (): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-cli-runner-'))
  tempDirs.push(dir)
  return dir
}
