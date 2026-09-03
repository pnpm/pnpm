import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { PnpmError } from '@pnpm/error'
import { exec, run } from '@pnpm/exec.commands'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import type { ProjectRootDir, ProjectRootDirRealPath } from '@pnpm/types'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { safeExeca as execa } from 'execa'

import { DEFAULT_OPTS, REGISTRY_URL } from './utils/index.js'

const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')
const testOnPosixOnly = process.platform === 'win32' ? test.skip : test

test('pnpm recursive exec', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: `${server1.sendLineScript('project-1')} && ${server2.sendLineScript('project-1')}`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server1.sendLineScript('project-2'),
        postbuild: server1.sendLineScript('project-2-postbuild'),
        prebuild: server1.sendLineScript('project-2-prebuild'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server2.sendLineScript('project-3'),
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, ['npm', 'run', 'build'])

  expect(server1.getLines()).toStrictEqual(['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  expect(server2.getLines()).toStrictEqual(['project-1', 'project-3'])
})

test('pnpm recursive exec finds bin files of workspace projects', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      dependencies: {
        cowsay: '1.5.0',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        cowsay: '1.5.0',
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, ['cowsay', 'hi'])

  // If there was no exception, the test passed
})

test('exec inside a workspace package', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: `${server1.sendLineScript('project-1')} && ${server2.sendLineScript('project-1')}`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server1.sendLineScript('project-2'),
        postbuild: server1.sendLineScript('project-2-postbuild'),
        prebuild: server1.sendLineScript('project-2-prebuild'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server2.sendLineScript('project-3'),
      },
    },
  ])

  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  process.chdir('project-1')
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: path.resolve('project-1'),
    recursive: false,
    selectedProjectsGraph: {},
  }, ['npm', 'run', 'build'])

  expect(server1.getLines()).toStrictEqual(['project-1'])
  expect(server2.getLines()).toStrictEqual(['project-1'])
})

test('pnpm recursive exec sets PNPM_PACKAGE_NAME env var', async () => {
  preparePackages([
    {
      name: 'foo',
      version: '1.0.0',
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, ['node', '-e', 'require(\'fs\').writeFileSync(\'pkgname\', process.env.PNPM_PACKAGE_NAME, \'utf8\')'])

  expect(fs.readFileSync('foo/pkgname', 'utf8')).toBe('foo')
})

test('testing the bail config with "pnpm recursive exec"', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: `exit 1 && ${server.sendLineScript('project-2')}`,
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server.sendLineScript('project-3'),
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  let failed = false
  let err1!: PnpmError
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
    }, ['npm', 'run', 'build'])
  } catch (_err: any) { // eslint-disable-line
    err1 = _err
    failed = true
  }
  expect(err1.code).toBe('ERR_PNPM_RECURSIVE_FAIL')
  expect(failed).toBeTruthy()

  expect(server.getLines()).toStrictEqual(['project-1', 'project-3'])

  failed = false
  let err2!: PnpmError
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: true,
      selectedProjectsGraph,
    }, ['npm', 'run', 'build'])
  } catch (_err: any) { // eslint-disable-line
    err2 = _err
    failed = true
  }

  expect(err2.code).toBe('ERR_PNPM_RECURSIVE_FAIL')
  expect(failed).toBeTruthy()
})

test('without --bail, recursive exec skips dependents of a failed project and runs unrelated ones', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      dependencies: {
        'project-b': 'workspace:*',
      },
      scripts: {
        build: server.sendLineScript('project-a'),
      },
    },
    {
      name: 'project-b',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
    {
      name: 'project-c',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('project-c'),
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  let err!: PnpmError
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: true,
      reportSummary: true,
      selectedProjectsGraph,
      workspaceDir: process.cwd(),
    }, ['npm', 'run', 'build'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  // The failure that blocked project-a is already counted; the skipped
  // dependent must not turn one failure into two.
  expect(err.code).toBe('ERR_PNPM_RECURSIVE_FAIL')
  expect(err.message).toContain('failed in 1 packages')
  expect(server.getLines()).toStrictEqual(['project-c'])
  const { default: { executionStatus } } = (await import(path.resolve('pnpm-exec-summary.json'), { with: { type: 'json' } }))
  expect(executionStatus[path.resolve('project-a')].status).toBe('skipped')
  expect(executionStatus[path.resolve('project-b')].status).toBe('failure')
  expect(executionStatus[path.resolve('project-c')].status).toBe('passed')
})

test('pnpm recursive exec --no-sort', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'a-dependent',
      version: '1.0.0',

      dependencies: {
        'b-dependency': 'workspace:*',
      },
      scripts: {
        build: server.sendLineScript('a-dependent'),
      },
    },
    {
      name: 'b-dependency',
      version: '1.0.0',

      scripts: {
        build: server.sendLineScript('b-dependency'),
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    sort: false,
    workspaceConcurrency: 1,
  }, ['npm', 'run', 'build'])

  expect(server.getLines()).toStrictEqual(['a-dependent', 'b-dependency'])
})

test('pnpm recursive exec --reverse', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server.sendLineScript('project-2'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server.sendLineScript('project-3'),
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    selectedProjectsGraph,
    recursive: true,
    sort: true,
    reverse: true,
  }, ['npm', 'run', 'build'])

  const outputs1 = server.getLines()

  expect(outputs1[outputs1.length - 1]).toBe('project-1')
})

test('pnpm exec on single project', async () => {
  prepare({})

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: false,
    selectedProjectsGraph: {},
  }, ['node', '-e', 'require("fs").writeFileSync("output.json", "[]", "utf8")'])

  const { default: outputs } = await import(path.resolve('output.json'))
  expect(outputs).toStrictEqual([])
})

test('pnpm exec on single project should return non-zero exit code when the process fails', async () => {
  prepare({})

  {
    const { exitCode } = await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: false,
      selectedProjectsGraph: {},
    }, ['node', '-e', 'process.exitCode=1'])

    expect(exitCode).toBe(1)
  }

  {
    const runResult = await run.handler({
      ...DEFAULT_OPTS,
      argv: {
        original: ['pnpm', 'node', '-e', 'process.exitCode=1'],
      },
      dir: process.cwd(),
      fallbackCommandUsed: true,
      recursive: false,
      selectedProjectsGraph: {},
    }, ['node'])

    expect(runResult).toHaveProperty(['exitCode'], 1)
  }
})

test('pnpm exec outside of projects', async () => {
  prepareEmpty()

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: false,
    selectedProjectsGraph: {},
  }, ['node', '-e', 'require("fs").writeFileSync("output.json", "[]", "utf8")'])

  const { default: outputs } = await import(path.resolve('output.json'))
  expect(outputs).toStrictEqual([])
})

test('pnpm exec shell mode', async () => {
  prepareEmpty()

  const echoArgs = process.platform === 'win32' ? '%PNPM_PACKAGE_NAME% > name.txt' : '$PNPM_PACKAGE_NAME > name.txt'

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: false,
    selectedProjectsGraph: {
      [process.cwd() as ProjectRootDir]: {
        dependencies: [],
        package: {
          rootDir: process.cwd() as ProjectRootDir,
          rootDirRealPath: process.cwd() as ProjectRootDirRealPath,
          writeProjectManifest: async () => {},
          manifest: {
            name: 'test_shell_mode',
          },
        },
      },
    },
    shellMode: true,
  }, ['echo', echoArgs])

  const result = (fs.readFileSync(path.resolve('name.txt'), 'utf8')).trim()

  expect(result).toBe('test_shell_mode')
})

// This test is not stable on Windows
testOnPosixOnly('pnpm recursive exec works with PnP', async () => {
  await using server1 = await createTestIpcServer()
  await using server2 = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',

      scripts: {
        build: `${server1.sendLineScript('project-1')} && ${server2.sendLineScript('project-1')}`,
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server1.sendLineScript('project-2'),
        postbuild: server1.sendLineScript('project-2-postbuild'),
        prebuild: server1.sendLineScript('project-2-prebuild'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',

      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server2.sendLineScript('project-3'),
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ], {
    env: {
      pnpm_config_node_linker: 'pnp',
      pnpm_config_symlink: 'false',
    },
  })
  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
  }, ['npm', 'run', 'build'])

  expect(server1.getLines()).toStrictEqual(['project-1', 'project-2-prebuild', 'project-2', 'project-2-postbuild'])
  expect(server2.getLines()).toStrictEqual(['project-1', 'project-3'])
})

test('pnpm recursive exec --resume-from should work', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('project-1'),
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server.sendLineScript('project-2'),
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      dependencies: {
        'project-1': '1',
      },
      scripts: {
        build: server.sendLineScript('project-3'),
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('project-4'),
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  await exec.handler({
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    selectedProjectsGraph,
    recursive: true,
    sort: true,
    resumeFrom: 'project-3',
  }, ['npm', 'run', 'build'])

  // Only the anchor's transitive dependencies (project-1) are skipped;
  // project-4 is unrelated to the anchor, so resuming still runs it.
  expect(server.getLines().sort()).toEqual(['project-2', 'project-3', 'project-4'])
})

test('recursive exec resumes from exactly the projects that passed before a failure', async () => {
  preparePackages([
    {
      name: 'dependency',
      version: '1.0.0',
    },
    {
      name: 'anchor',
      version: '1.0.0',
      dependencies: {
        dependency: '1',
      },
    },
    {
      name: 'completed',
      version: '1.0.0',
    },
  ])
  await fs.promises.writeFile('fail', '')
  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  const command = [
    'node',
    '-e',
    "const fs = require('fs'); const name = require('path').basename(process.cwd()); fs.appendFileSync('../order.log', `${name}\\n`); if (name === 'dependency' && fs.existsSync('../fail')) process.exit(1)",
  ]
  const opts = {
    ...DEFAULT_OPTS,
    bail: false,
    dir: process.cwd(),
    recursive: true,
    selectedProjectsGraph,
    sort: true,
    workspaceConcurrency: 1,
    workspaceDir: process.cwd(),
  }

  await expect(exec.handler(opts, command)).rejects.toMatchObject({ code: 'ERR_PNPM_RECURSIVE_FAIL' })
  const firstRun = (await fs.promises.readFile('order.log', 'utf8')).trim().split('\n')
  expect([...firstRun].sort()).toStrictEqual(['completed', 'dependency'])

  await fs.promises.rm('fail')
  await exec.handler({ ...opts, bail: true, resumeFrom: 'anchor' }, command)

  expect((await fs.promises.readFile('order.log', 'utf8')).trim().split('\n')).toStrictEqual([
    ...firstRun,
    'dependency',
    'anchor',
  ])
})

test('should throw error when the package specified by resume-from does not exist', async () => {
  preparePackages([
    {
      name: 'foo',
      version: '1.0.0',
      scripts: {
        build: 'echo foo',
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      selectedProjectsGraph,
      recursive: true,
      sort: true,
      resumeFrom: 'project-2',
    }, ['npm', 'run', 'build'])
  } catch (err: any) { // eslint-disable-line
    expect(err.code).toBe('ERR_PNPM_RESUME_FROM_NOT_FOUND')
  }
})

test('pnpm exec in directory with path delimiter', async () => {
  preparePackages([
    {
      name: `foo${path.delimiter}delimiter`,
      version: '1.0.0',
      dependencies: {
        cowsay: '1.5.0',
      },
    },
  ])

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  await execa(pnpmBin, [
    'install',
    '-r',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  let error
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      selectedProjectsGraph,
      recursive: true,
    }, ['cowsay', 'hi'])
  } catch (err: any) { // eslint-disable-line
    error = err
  }
  expect(error).toBeUndefined()
})

test('pnpm recursive exec report summary', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        build: 'node -e "setTimeout(() => console.log(\'project-1\'), 1000)"',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      scripts: {
        build: 'node -e "setTimeout(() => console.log(\'project-3\'), 1000)"',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
  ])
  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  let error
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      selectedProjectsGraph,
      recursive: true,
      reportSummary: true,
      workspaceConcurrency: 3,
    }, ['npm', 'run', 'build'])
  } catch (err: any) { // eslint-disable-line
    error = err
  }
  expect(error.code).toBe('ERR_PNPM_RECURSIVE_FAIL')

  const { default: { executionStatus } } = (await import(path.resolve('pnpm-exec-summary.json')))
  expect(executionStatus[path.resolve('project-1')].status).toBe('passed')
  expect(executionStatus[path.resolve('project-1')].duration).not.toBeFalsy()
  expect(executionStatus[path.resolve('project-2')].status).toBe('failure')
  expect(executionStatus[path.resolve('project-2')].duration).not.toBeFalsy()
  expect(executionStatus[path.resolve('project-3')].status).toBe('passed')
  expect(executionStatus[path.resolve('project-3')].duration).not.toBeFalsy()
  expect(executionStatus[path.resolve('project-4')].status).toBe('failure')
  expect(executionStatus[path.resolve('project-4')].duration).not.toBeFalsy()
})

test('pnpm recursive exec report summary with --bail', async () => {
  preparePackages([
    {
      name: 'project-1',
      version: '1.0.0',
      scripts: {
        build: 'node -e "setTimeout(() => console.log(\'project-1\'), 1000)"',
      },
    },
    {
      name: 'project-2',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
    {
      name: 'project-3',
      version: '1.0.0',
      scripts: {
        build: 'node -e "setTimeout(() => console.log(\'project-3\'), 1000)"',
      },
    },
    {
      name: 'project-4',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
      },
    },
  ])
  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  let error
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      selectedProjectsGraph,
      recursive: true,
      reportSummary: true,
      bail: true,
      workspaceConcurrency: 3,
    }, ['npm', 'run', 'build'])
  } catch (err: any) { // eslint-disable-line
    error = err
  }
  expect(error.code).toBe('ERR_PNPM_RECURSIVE_EXEC_FIRST_FAIL')

  const { default: { executionStatus } } = (await import(path.resolve('pnpm-exec-summary.json')))

  // The first failure ends the run at once: commands still running are
  // reported as such (the exit path terminates them), and a command still
  // queued behind the concurrency limit is never started.
  expect(executionStatus[path.resolve('project-1')].status).toBe('running')
  expect(executionStatus[path.resolve('project-2')].status).toBe('failure')
  expect(executionStatus[path.resolve('project-2')].duration).not.toBeFalsy()
  expect(executionStatus[path.resolve('project-3')].status).toBe('running')
  expect(executionStatus[path.resolve('project-4')].status).toBe('queued')
})

test('pnpm exec command not found (implicit fallback)', async () => {
  prepare({
    scripts: {
      build: 'echo hello',
    },
  })

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  let error!: Error & { hint?: string }
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: false,
      bail: true,
      selectedProjectsGraph,
      implicitlyFellbackFromRun: true,
    }, ['buil']) // cspell:disable-line
  } catch (err: any) { // eslint-disable-line
    error = err
  }
  expect(error?.message).toBe('Command "buil" not found') // cspell:disable-line
  expect(error?.hint).toBe('Did you mean "pnpm build"?')
})

test('pnpm exec command not found (explicit call, without near name packages)', async () => {
  prepare({
    scripts: {
      cwsay: 'echo hello',
    },
  })

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])
  let error!: Error & { hint?: string }
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: false,
      bail: true,
      selectedProjectsGraph,
      implicitlyFellbackFromRun: false,
    }, ['cwsay'])
  } catch (err: any) { // eslint-disable-line
    error = err
  }
  expect(error?.message).toBe('Command "cwsay" not found')
  expect(error?.hint).toBeFalsy()
})

test('pnpm exec command not found (explicit call, with a near name package)', async () => {
  prepare({
    dependencies: {
      cowsay: '1.5.0',
    },
  })

  const { selectedProjectsGraph } = await filterProjectsBySelectorObjectsFromDir(process.cwd(), [])

  await execa(pnpmBin, [
    'install',
    '--registry',
    REGISTRY_URL,
    '--store-dir',
    path.resolve(DEFAULT_OPTS.storeDir),
  ])

  let error!: Error & { hint?: string }
  try {
    await exec.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      recursive: false,
      bail: true,
      selectedProjectsGraph,
      implicitlyFellbackFromRun: false,
    }, ['cwsay'])
  } catch (err: any) { // eslint-disable-line
    error = err
  }
  expect(error?.message).toBe('Command "cwsay" not found')
  expect(error?.hint).toBe('Did you mean "pnpm exec cowsay"?')
})

test('pnpm exec --workspace-root when command not found', async () => {
  prepare({})

  let error!: any // eslint-disable-line
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      argv: {
        original: ['pnpm', '--workspace-root', 'command-that-does-not-exist'],
      },
      dir: process.cwd(),
      fallbackCommandUsed: true,
      recursive: false,
      selectedProjectsGraph: {},
    }, ['command-that-does-not-exist'])
  } catch (err: any) { // eslint-disable-line
    error = err
  }

  expect(error?.failures[0].message).toBe('Command "command-that-does-not-exist" not found')
})
