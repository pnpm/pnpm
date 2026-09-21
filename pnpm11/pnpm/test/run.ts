import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { killProcessGroup, prepare, preparePackages } from '@pnpm/prepare'
import isWindows from 'is-windows'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync, pnpmBinLocation, spawnPnpm, writeFakeBin } from './utils/index.js'

const RECORD_ARGS_FILE = 'require(\'fs\').writeFileSync(\'args.json\', JSON.stringify(require(\'./args.json\').concat([process.argv.slice(2)])), \'utf8\')'
const testOnPosix = isWindows() ? test.skip : test

test('run -r: pass the args to the command that is specified in the build script', async () => {
  preparePackages([{
    name: 'project',
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  }])
  fs.writeFileSync('project/args.json', '[]', 'utf8')
  fs.writeFileSync('project/recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm(['run', '-r', '--config.enable-pre-post-scripts', '--config.verify-deps-before-run=false', 'foo', 'arg', '--flag=true'])

  const { default: args } = await import(path.resolve('project/args.json'))
  expect(args).toStrictEqual([
    [],
    ['arg', '--flag=true'],
    [],
  ])
})

test('run: pass the args to the command that is specified in the build script', async () => {
  prepare({
    name: 'project',
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  })
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm(['run', 'foo', 'arg', '--flag=true'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([
    [],
    ['arg', '--flag=true'],
    [],
  ])
})

// Before pnpm v7, `--` was required to pass flags to a build script. Now all
// arguments after the script name should be passed to the build script, even
// `--`.
test('run: pass all arguments after script name to the build script, even --', async () => {
  prepare({
    name: 'project',
    scripts: {
      foo: 'node recordArgs',
      postfoo: 'node recordArgs',
      prefoo: 'node recordArgs',
    },
  })
  fs.writeFileSync('args.json', '[]', 'utf8')
  fs.writeFileSync('recordArgs.js', RECORD_ARGS_FILE, 'utf8')

  await execPnpm(['run', 'foo', 'arg', '--', '--flag=true'])

  const { default: args } = await import(path.resolve('args.json'))
  expect(args).toStrictEqual([
    [],
    ['arg', '--', '--flag=true'],
    [],
  ])
})

test('exit code of child process is preserved', async () => {
  prepare({
    scripts: {
      foo: 'exit 87',
    },
  })
  const result = execPnpmSync(['run', 'foo'])
  expect(result.status).toBe(87)
})

test('recursive test: pass the args to the command that is specified in the build script of a package.json manifest', async () => {
  preparePackages([{
    name: 'project',
    scripts: {
      test: 'ts-node test',
    },
  }])

  const result = execPnpmSync(['--config.verify-deps-before-run=false', '-r', 'test', 'arg', '--flag=true'])

  expect((result.stdout as Buffer).toString('utf8')).toMatch(
    process.platform === 'win32' ? /ts-node test "arg" "--flag=true"/ : /ts-node test arg --flag=true/
  )
})

test('start: run "node server.js" by default', async () => {
  prepare({}, { manifestFormat: 'YAML' })

  fs.writeFileSync('server.js', 'console.log("Hello world!")', 'utf8')

  const result = execPnpmSync(['start'])

  expect((result.stdout as Buffer).toString('utf8')).toMatch(/Hello world!/)
})

test('install-test: install dependencies and runs tests', async () => {
  prepare({
    scripts: {
      test: 'node -e "process.stdout.write(\'test\')" > ./output.txt',
    },
  }, { manifestFormat: 'JSON5' })

  await execPnpm(['install-test'])

  const scriptsRan = (fs.readFileSync('output.txt')).toString()
  expect(scriptsRan.trim()).toBe('test')
})

test.each(['--no-bail', '--bail=false'])('install-test: %s continues after a workspace test fails', (bailOption) => {
  preparePackages([
    { name: 'project-1', scripts: { test: 'node test.cjs' } },
    { name: 'project-2', scripts: { test: 'node test.cjs' } },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'], workspaceConcurrency: 1 })
  fs.writeFileSync('project-1/test.cjs', "require('fs').appendFileSync('../order.txt', 'first\\n'); process.exit(1)")
  fs.writeFileSync('project-2/test.cjs', "require('fs').appendFileSync('../order.txt', 'second\\n')")

  const result = execPnpmSync(['-r', bailOption, 'install-test'])

  expect(result.status).toBe(1)
  expect(fs.readFileSync('order.txt', 'utf8')).toBe('first\nsecond\n')
})

test('silent run only prints the output of the child process', async () => {
  prepare({
    scripts: {
      hi: 'echo hi && exit 1',
    },
  })

  const result = execPnpmSync(['run', '--silent', '--config.verify-deps-before-run=false', 'hi'])

  expect(result.stdout.toString().trim()).toBe('hi')
})

test('silent run does not print verifyDepsBeforeRun install output', async () => {
  prepare({
    scripts: {
      hi: 'echo hi',
    },
  })
  writeYamlFileSync('pnpm-workspace.yaml', {
    verifyDepsBeforeRun: 'install',
  })

  const result = execPnpmSync(['run', '--silent', 'hi'], {
    expectSuccess: true,
    omitEnvDefaults: ['pnpm_config_silent'],
  })

  expect(result.stdout.toString().trim()).toBe('hi')
})

testOnPosix('pnpm run with preferSymlinkedExecutables true', async () => {
  prepare({
    scripts: {
      build: 'node -e "console.log(process.env.NODE_PATH)"',
    },
  })

  writeYamlFileSync('pnpm-workspace.yaml', {
    preferSymlinkedExecutables: true,
  })

  const result = execPnpmSync(['run', 'build'])

  expect(result.stdout.toString()).toContain(`project${path.sep}node_modules${path.sep}.pnpm${path.sep}node_modules`)
})

testOnPosix('pnpm run with preferSymlinkedExecutables and custom virtualStoreDir', async () => {
  prepare({
    scripts: {
      build: 'node -e "console.log(process.env.NODE_PATH)"',
    },
  })

  writeYamlFileSync('pnpm-workspace.yaml', {
    virtualStoreDir: '/foo/bar',
    preferSymlinkedExecutables: true,
  })

  const result = execPnpmSync(['run', 'build'])

  expect(result.stdout.toString()).toContain(`${path.sep}foo${path.sep}bar${path.sep}node_modules`)
})

test('collapse output when running multiple scripts in one project', async () => {
  prepare({
    scripts: {
      script1: 'echo 1',
      script2: 'echo 2',
    },
  })

  const result = execPnpmSync(['run', '/script[12]/'])

  const output = result.stdout.toString()
  expect(output).toContain('script1: 1')
  expect(output).toContain('script2: 2')
})

test('do not collapse output when running multiple scripts in one project sequentially', async () => {
  prepare({
    scripts: {
      script1: 'echo 1',
      script2: 'echo 2',
    },
  })

  const result = execPnpmSync(['--workspace-concurrency=1', 'run', '/script[12]/'])

  const output = result.stdout.toString()
  expect(output).not.toContain('script1: 1')
  expect(output).not.toContain('script2: 2')
})

test('--parallel should work with single project', async () => {
  prepare({
    scripts: {
      script1: 'echo 1',
      script2: 'echo 2',
    },
  })

  const result = execPnpmSync(['--parallel', 'run', '/script[12]/'])

  const output = result.stdout.toString()
  expect(output).toContain('script1: 1')
  expect(output).toContain('script2: 2')
})

test('--reporter-hide-prefix should hide workspace prefix', async () => {
  prepare({
    scripts: {
      script1: 'echo 1',
      script2: 'echo 2',
    },
  })

  const result = execPnpmSync(['--parallel', '--reporter-hide-prefix', 'run', '/script[12]/'])

  const output = result.stdout.toString()
  expect(output).toContain('1')
  expect(output).not.toContain('script1: 1')
  expect(output).toContain('2')
  expect(output).not.toContain('script2: 2')
})

test('hidden scripts (starting with .) cannot be run directly', () => {
  prepare({
    scripts: {
      '.build': 'echo hidden',
      'build': 'pnpm run .build',
    },
  })

  const result = execPnpmSync(['run', '.build'])
  expect(result.status).toBe(1)
  const output = result.stdout.toString() + result.stderr.toString()
  expect(output).toContain('HIDDEN_SCRIPT')
})

test('hidden scripts can be called from other scripts', () => {
  prepare({
    scripts: {
      '.build': 'echo hidden-ok',
      'build': 'pnpm run .build',
    },
  })

  const result = execPnpmSync(['run', 'build'])
  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('hidden-ok')
})

test('hidden scripts are not shown in pnpm run listing', () => {
  prepare({
    scripts: {
      '.internal': 'echo hidden',
      'build': 'echo visible',
    },
  })

  const result = execPnpmSync(['run'])
  expect(result.stdout.toString()).toContain('build')
  expect(result.stdout.toString()).not.toContain('.internal')
})

test('regex selector skips hidden scripts', () => {
  prepare({
    scripts: {
      '.build-internal': 'echo hidden',
      'build': 'echo visible',
    },
  })

  const result = execPnpmSync(['run', '/build/'])
  expect(result.status).toBe(0)
  expect(result.stdout.toString()).toContain('visible')
  expect(result.stdout.toString()).not.toContain('hidden')
})

// A script that reads a repeated interrupt as an order to stop at once, as
// many CLIs do: the first starts a graceful shutdown, the second forces an exit.
const COUNTING_SCRIPT = `const fs = require('node:fs')
let interrupts = 0
process.on('SIGINT', () => {
  interrupts += 1
  if (interrupts > 1) {
    fs.writeFileSync('forced.txt', '')
    process.exit(130)
  }
  setTimeout(() => {
    fs.writeFileSync('shut-down.txt', '')
    process.exit(0)
  }, 1000)
})
fs.writeFileSync('started.txt', '')
console.log('started')
setInterval(() => {}, 1000)
`

// Ctrl+C interrupts the terminal's whole foreground group, so the script has
// the signal by the time pnpm does. pnpm passes nothing on, and the script
// counts one interrupt rather than two.
// https://github.com/pnpm/pnpm/issues/7374
testOnPosix('run: Ctrl+C in a terminal interrupts the script once', () => {
  prepare({
    name: 'project',
    scripts: {
      dev: 'exec node dev.js',
    },
  })
  fs.writeFileSync('dev.js', COUNTING_SCRIPT, 'utf8')

  const terminalScript = path.join(import.meta.dirname, '../../__utils__/scripts/terminal.py')
  const { stdout, status, error } = spawnSync('python3', [
    terminalScript,
    process.execPath,
    pnpmBinLocation,
    'run',
    '--config.verify-deps-before-run=false',
    'dev',
  ], { encoding: 'utf8', timeout: 90_000 })

  expect(error).toBeUndefined()
  expect(fs.existsSync('forced.txt')).toBe(false)
  expect(fs.existsSync('shut-down.txt')).toBe(true)
  expect(status).toBe(0)
  expect(stdout).toContain('started')
})

testOnPosix('run -r: Ctrl+C does not report interrupted scripts as failures', () => {
  preparePackages([
    {
      name: 'project-1',
      scripts: {
        dev: 'node ../dev.js',
      },
    },
    {
      name: 'project-2',
      scripts: {
        dev: 'node ../dev.js',
      },
    },
  ])
  fs.writeFileSync('dev.js', `const fs = require('node:fs')
fs.appendFileSync('../started.txt', 'x')
if (fs.readFileSync('../started.txt', 'utf8').length === 2) console.log('started')
setInterval(() => {}, 1000)
`, 'utf8')
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  const terminalScript = path.join(import.meta.dirname, '../../__utils__/scripts/terminal.py')
  const { stdout, status, error } = spawnSync('python3', [
    terminalScript,
    process.execPath,
    pnpmBinLocation,
    'run',
    '-r',
    '--stream',
    '--config.verify-deps-before-run=false',
    'dev',
  ], { encoding: 'utf8', timeout: 90_000 })

  expect(error).toBeUndefined()
  expect(status).toBe(130)
  expect(stdout).not.toContain('ELIFECYCLE')
  expect(stdout).not.toContain('ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL')
})

testOnPosix('run -r: Ctrl+C stops dispatch while interrupted scripts settle', () => {
  preparePackages([
    {
      name: 'project-1',
      scripts: {
        dev: 'node ../exit-cleanly.js',
      },
    },
    {
      name: 'project-2',
      scripts: {
        dev: 'node ../exit-cleanly-too.js',
      },
    },
    {
      name: 'project-3',
      scripts: {
        dev: 'node ../stay-running.js',
      },
    },
  ])
  fs.writeFileSync('exit-cleanly.js', `const fs = require('node:fs')
process.on('SIGINT', () => process.exit(0))
fs.appendFileSync('../started.txt', 'x')
if (fs.readFileSync('../started.txt', 'utf8').length === 2) console.log('started')
setInterval(() => {}, 1000)
`, 'utf8')
  fs.writeFileSync('exit-cleanly-too.js', `const fs = require('node:fs')
process.on('SIGINT', () => process.exit(0))
fs.appendFileSync('../started.txt', 'x')
if (fs.readFileSync('../started.txt', 'utf8').length === 2) console.log('started')
setInterval(() => {}, 1000)
`, 'utf8')
  fs.writeFileSync('stay-running.js', `const fs = require('node:fs')
fs.writeFileSync('../started-late.txt', '')
setInterval(() => {}, 1000)
`, 'utf8')
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['project-*'] })

  const terminalScript = path.join(import.meta.dirname, '../../__utils__/scripts/terminal.py')
  const { stdout, status, error } = spawnSync('python3', [
    terminalScript,
    process.execPath,
    pnpmBinLocation,
    'run',
    '-r',
    '--stream',
    '--workspace-concurrency=2',
    '--config.verify-deps-before-run=false',
    'dev',
  ], { encoding: 'utf8', timeout: 90_000 })

  expect(error).toBeUndefined()
  expect(fs.existsSync('started-late.txt')).toBe(false)
  expect(status).toBe(130)
  expect(stdout).not.toContain('ELIFECYCLE')
  expect(stdout).not.toContain('ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL')
})

// A script that shuts down on SIGTERM the way a server does when a container
// runtime stops it.
const TERMINATING_SCRIPT = `const fs = require('node:fs')
process.on('SIGTERM', () => {
  setTimeout(() => {
    fs.writeFileSync('shut-down.txt', '')
    process.exit(0)
  }, 1000)
})
fs.writeFileSync('started.txt', '')
setInterval(() => {}, 1000)
`

// Without a terminal, the shell running the script may stay its parent (dash
// does) and dies from SIGTERM at once. pnpm signals the script's whole process
// group instead and waits for it, so the script finishes shutting down.
testOnPosix('run: a SIGTERM sent to pnpm without a terminal reaches the script behind its shell', async () => {
  prepare({
    name: 'project',
    scripts: {
      dev: 'node dev.js',
    },
  })
  fs.writeFileSync('dev.js', TERMINATING_SCRIPT, 'utf8')

  const proc = spawnPnpm(['run', '--config.verify-deps-before-run=false', 'dev'], { detached: true })
  // The script may outlive pnpm and keep the output pipes open, so the
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
  } finally {
    killProcessGroup(proc.pid!)
  }
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

test('run resolves commands from the configured modules directory, not a stale node_modules/.bin', async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
    scripts: { greet: 'greet' },
  })
  writeYamlFileSync('pnpm-workspace.yaml', { modulesDir: 'vendor' })
  writeFakeBin(path.resolve('vendor/.bin'), 'greet', 'configured')
  writeFakeBin(path.resolve('node_modules/.bin'), 'greet', 'stale')

  const result = execPnpmSync(['run', '--config.verify-deps-before-run=false', 'greet'])

  expect(result.status).toBe(0)
  const stdout = result.stdout.toString()
  expect(stdout).toContain('configured')
  expect(stdout).not.toContain('stale')
})

test('run resolves a command from the modules directory a packageConfigs entry gives the project', async () => {
  preparePackages([
    { location: '.', package: { name: 'root', version: '1.0.0' } },
    { name: 'moved', version: '1.0.0', scripts: { greet: 'greet' } },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**', '!store/**'],
    modulesDir: 'vendor',
    sharedWorkspaceLockfile: false,
    packageConfigs: { moved: { modulesDir: 'node_modules' } },
  })
  writeFakeBin(path.resolve('moved/node_modules/.bin'), 'greet', 'configured')
  writeFakeBin(path.resolve('moved/vendor/.bin'), 'greet', 'stale')

  const result = execPnpmSync(['run', '--config.verify-deps-before-run=false', 'greet'], {
    cwd: path.resolve('moved'),
  })

  expect(result.status).toBe(0)
  const stdout = result.stdout.toString()
  expect(stdout).toContain('configured')
  expect(stdout).not.toContain('stale')

  // `pnpm -r run` reaches the script through runRecursive, which resolves the
  // entry of every project it visits rather than of the one it started in.
  const recursive = execPnpmSync(['-r', 'run', '--config.verify-deps-before-run=false', 'greet'])

  expect(recursive.status).toBe(0)
  const recursiveStdout = recursive.stdout.toString()
  expect(recursiveStdout).toContain('configured')
  expect(recursiveStdout).not.toContain('stale')
})

test('run and exec reach plugins installed in the configured modules directory', async () => {
  prepare({
    name: 'root',
    version: '1.0.0',
    scripts: { lint: 'tool' },
    dependencies: { plugin: 'file:plugin', tool: 'file:tool' },
  })
  writeYamlFileSync('pnpm-workspace.yaml', { modulesDir: 'vendor' })
  fs.mkdirSync('tool')
  fs.writeFileSync('tool/package.json', JSON.stringify({ name: 'tool', version: '1.0.0', bin: 'bin.js' }))
  // Loads plugins from the working directory, the way ESLint and similar tools do.
  fs.writeFileSync('tool/bin.js', `#!/usr/bin/env node
const { createRequire } = require('node:module')
console.log(createRequire(require('node:path').join(process.cwd(), 'package.json'))('plugin'))
`)
  fs.mkdirSync('plugin')
  fs.writeFileSync('plugin/package.json', JSON.stringify({ name: 'plugin', version: '1.0.0' }))
  fs.writeFileSync('plugin/index.js', 'module.exports = \'plugin loaded\'\n')

  await execPnpm(['install'])

  for (const args of [['run', 'lint'], ['exec', 'tool']]) {
    const result = execPnpmSync(args)
    expect(result.status).toBe(0)
    expect(result.stdout.toString()).toContain('plugin loaded')
  }
})
