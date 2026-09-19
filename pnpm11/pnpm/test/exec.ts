import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { killProcessGroup, prepare, preparePackages } from '@pnpm/prepare'
import isWindows from 'is-windows'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync, pnpmBinLocation, spawnPnpm } from './utils/index.js'

test("exec should respect the caller's current working directory", async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
  })

  const projectRoot = process.cwd()
  fs.mkdirSync('some-directory', { recursive: true })
  const subdirPath = path.join(projectRoot, 'some-directory')

  await execPnpm(['install'])

  const cmdFilePath = path.join(subdirPath, 'cwd.txt')

  execPnpmSync(
    ['exec', 'node', '-e', `require('fs').writeFileSync(${JSON.stringify(cmdFilePath)}, process.cwd(), 'utf8')`],
    {
      cwd: subdirPath,
      expectSuccess: true,
    }
  )

  expect(fs.readFileSync(cmdFilePath, 'utf8')).toBe(subdirPath)
})

test('exec and the run fallback find the project bins from a subdirectory', async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
    dependencies: {
      '@pnpm.e2e/hello-world-js-bin': '1.0.0',
    },
  })
  await execPnpm(['install'])
  const subdirPath = path.join(process.cwd(), 'some-directory')
  fs.mkdirSync(subdirPath)

  for (const args of [['exec', 'hello-world-js-bin'], ['hello-world-js-bin']]) {
    const result = execPnpmSync(args, { cwd: subdirPath, expectSuccess: true })
    expect(result.stdout.toString()).toContain('Hello world!')
  }
})

test('silent exec does not print verifyDepsBeforeRun install output', async () => {
  prepare({})
  writeYamlFileSync('pnpm-workspace.yaml', {
    verifyDepsBeforeRun: 'install',
  })

  const result = execPnpmSync(['--silent', 'exec', 'node', '-e', 'process.stdout.write("hi")'], {
    expectSuccess: true,
    omitEnvDefaults: ['pnpm_config_silent'],
  })

  expect(result.stdout.toString()).toBe('hi')
})

const testOnPosix = isWindows() ? test.skip : test

// A command that shuts down on the first SIGINT or SIGTERM and exits at once
// on a second interrupt, the way many CLIs treat a repeated Ctrl+C. Every
// signal it gets is appended to signals.txt, so a test can tell one SIGINT
// from a SIGINT followed by a SIGTERM.
const SHUTTING_DOWN_COMMAND = `const fs = require('node:fs')
let interrupts = 0
const shutDown = () => {
  setTimeout(() => {
    fs.writeFileSync('shut-down.txt', '')
    process.exit(0)
  }, 1000)
}
process.on('SIGTERM', () => {
  fs.appendFileSync('signals.txt', 'SIGTERM\\n')
  shutDown()
})
process.on('SIGINT', () => {
  fs.appendFileSync('signals.txt', 'SIGINT\\n')
  interrupts += 1
  if (interrupts > 1) {
    fs.writeFileSync('forced.txt', '')
    process.exit(130)
  }
  shutDown()
})
fs.writeFileSync('started.txt', '')
console.log('started')
setInterval(() => {}, 1000)
`

// Ctrl+C interrupts the terminal's whole foreground group, so the command has
// the signal by the time pnpm does. pnpm waits for it to finish shutting down
// instead of dying and taking the command with it.
testOnPosix('exec: Ctrl+C in a terminal lets the command finish shutting down', () => {
  prepare()
  fs.writeFileSync('dev.js', SHUTTING_DOWN_COMMAND, 'utf8')

  const terminalScript = path.join(import.meta.dirname, '../../__utils__/scripts/terminal.py')
  const { status, error } = spawnSync('python3', [
    terminalScript,
    process.execPath,
    pnpmBinLocation,
    'exec',
    'node',
    'dev.js',
  ], { encoding: 'utf8', timeout: 30_000 })

  expect(error).toBeUndefined()
  expect(fs.readFileSync('signals.txt', 'utf8')).toBe('SIGINT\n')
  expect(fs.existsSync('shut-down.txt')).toBe(true)
  expect(status).toBe(0)
})

testOnPosix('exec: a command that fails after Ctrl+C keeps its exit code', () => {
  prepare()
  fs.writeFileSync('dev.js', SHUTTING_DOWN_COMMAND.replace('process.exit(0)', 'process.exit(3)'), 'utf8')

  const terminalScript = path.join(import.meta.dirname, '../../__utils__/scripts/terminal.py')
  const { status, error } = spawnSync('python3', [
    terminalScript,
    process.execPath,
    pnpmBinLocation,
    'exec',
    'node',
    'dev.js',
  ], { encoding: 'utf8', timeout: 30_000 })

  expect(error).toBeUndefined()
  expect(fs.existsSync('shut-down.txt')).toBe(true)
  expect(status).toBe(3)
})

testOnPosix('exec: a SIGTERM sent to pnpm without a terminal reaches the command', async () => {
  prepare()
  fs.writeFileSync('dev.js', SHUTTING_DOWN_COMMAND, 'utf8')

  const proc = spawnPnpm(['exec', 'node', 'dev.js'], { detached: true })
  // The command may outlive pnpm and keep the output pipes open, so the
  // check is made the moment pnpm exits, not when its output closes.
  const shutDownBeforeExit = new Promise<boolean>((resolve) => {
    proc.on('exit', () => {
      resolve(fs.existsSync('shut-down.txt'))
    })
  })
  try {
    await waitForFile('started.txt', 30_000)
    proc.kill('SIGTERM')
    expect(await withDeadline(shutDownBeforeExit, 30_000)).toBe(true)
    expect(fs.readFileSync('signals.txt', 'utf8')).toBe('SIGTERM\n')
  } finally {
    killProcessGroup(proc.pid!)
  }
})

testOnPosix('exec -r: Ctrl+C stops the queued commands from starting', () => {
  preparePackages([
    { name: 'project-1', version: '1.0.0' },
    { name: 'project-2', version: '1.0.0' },
  ])
  for (const project of ['project-1', 'project-2']) {
    fs.writeFileSync(path.join(project, 'dev.js'), SHUTTING_DOWN_COMMAND, 'utf8')
  }

  const terminalScript = path.join(import.meta.dirname, '../../__utils__/scripts/terminal.py')
  const { stdout, status, error } = spawnSync('python3', [
    terminalScript,
    process.execPath,
    pnpmBinLocation,
    '-r',
    '--workspace-concurrency=1',
    '--config.verify-deps-before-run=false',
    'exec',
    'node',
    'dev.js',
  ], { encoding: 'utf8', timeout: 60_000 })

  expect(error).toBeUndefined()
  expect(stdout).toContain('started')
  expect(fs.existsSync('project-1/shut-down.txt')).toBe(true)
  expect(fs.existsSync('project-2/started.txt')).toBe(false)
  expect(status).toBe(130)
})

async function withDeadline<T> (promise: Promise<T>, timeout: number): Promise<T> {
  let timer: NodeJS.Timeout | undefined
  const deadline = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`pnpm did not exit within ${timeout}ms`)), timeout)
  })
  try {
    return await Promise.race([promise, deadline])
  } finally {
    clearTimeout(timer)
  }
}

async function waitForFile (file: string, timeout: number): Promise<void> {
  const deadline = Date.now() + timeout
  while (!fs.existsSync(file)) {
    if (Date.now() > deadline) throw new Error(`${file} did not appear within ${timeout}ms`)
    await new Promise<void>((resolve) => setTimeout(resolve, 50)) // eslint-disable-line no-await-in-loop
  }
}
