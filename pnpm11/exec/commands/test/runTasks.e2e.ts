import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import type { PnpmError } from '@pnpm/error'
import { run } from '@pnpm/exec.commands'
import { preparePackages } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { filterProjectsBySelectorObjectsFromDir } from '@pnpm/workspace.projects-filter'
import { safeExeca as execa } from 'execa'
import { writeYamlFileSync } from 'write-yaml-file'

import { DEFAULT_OPTS } from './utils/index.js'

const pnpmBin = path.join(import.meta.dirname, '../../../pnpm/bin/pnpm.mjs')

test('a task starts as soon as its dependencies finish, without waiting for unrelated tasks', async () => {
  await using server = await createTestIpcServer()

  // slow waits for a marker only mid writes, and mid may start only once
  // dep is done — so the run completes only if mid is dispatched while the
  // unrelated slow is still in flight. Any barrier between
  // dependency-independent tasks deadlocks this fixture.
  preparePackages([
    {
      name: 'dep',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('dep'),
      },
    },
    {
      name: 'mid',
      version: '1.0.0',
      dependencies: {
        dep: 'workspace:*',
      },
      scripts: {
        build: `${server.sendLineScript('mid')} && node -e "require('fs').writeFileSync('../slow-marker', '')"`,
      },
    },
    {
      name: 'slow',
      version: '1.0.0',
      scripts: {
        build: `node -e "const fs = require('fs'); const started = Date.now(); (function poll () { if (fs.existsSync('../slow-marker')) process.exit(0); if (Date.now() - started > 30000) process.exit(1); setTimeout(poll, 50) })()" && ${server.sendLineScript('slow')}`,
      },
    },
  ])

  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceConcurrency: 2,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(server.getLines()).toStrictEqual(['dep', 'mid', 'slow'])
})

test('dependsOn runs the tasks a task depends on, in dependency order', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      dependencies: {
        'project-b': 'workspace:*',
      },
      scripts: {
        build: server.sendLineScript('a-build'),
        test: server.sendLineScript('a-test'),
      },
    },
    {
      name: 'project-b',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('b-build'),
        test: server.sendLineScript('b-test'),
      },
    },
  ])

  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    tasks: {
      build: { dependsOn: ['^build'] },
      test: { dependsOn: ['build'] },
    },
    workspaceDir: process.cwd(),
  }, ['test'])

  const order = server.getLines()
  expect([...order].sort()).toStrictEqual(['a-build', 'a-test', 'b-build', 'b-test'])
  expect(order.indexOf('b-build')).toBeLessThan(order.indexOf('a-build'))
  expect(order.indexOf('a-build')).toBeLessThan(order.indexOf('a-test'))
  expect(order.indexOf('b-build')).toBeLessThan(order.indexOf('b-test'))
})

test('a task with an explicitly empty dependsOn starts without waiting for anything', async () => {
  await using server = await createTestIpcServer()

  // dependency's lint waits for the marker dependent's lint writes: only
  // possible when lint tasks are not ordered by the project graph.
  preparePackages([
    {
      name: 'dependency',
      version: '1.0.0',
      scripts: {
        lint: `node -e "const fs = require('fs'); const started = Date.now(); (function poll () { if (fs.existsSync('../lint-marker')) process.exit(0); if (Date.now() - started > 30000) process.exit(1); setTimeout(poll, 50) })()" && ${server.sendLineScript('dependency')}`,
      },
    },
    {
      name: 'dependent',
      version: '1.0.0',
      dependencies: {
        dependency: 'workspace:*',
      },
      scripts: {
        lint: `${server.sendLineScript('dependent')} && node -e "require('fs').writeFileSync('../lint-marker', '')"`,
      },
    },
  ])

  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    tasks: {
      lint: {},
    },
    workspaceConcurrency: 2,
    workspaceDir: process.cwd(),
  }, ['lint'])

  expect(server.getLines()).toStrictEqual(['dependent', 'dependency'])
})

test('a task concurrency limit applies across workspace projects', async () => {
  preparePackages(['project-a', 'project-b', 'project-c'].map((name) => ({
    name,
    version: '1.0.0',
    scripts: {
      build: `node ../concurrency-probe.cjs ${name}`,
    },
  })))
  fs.writeFileSync(path.join(process.cwd(), 'concurrency-probe.cjs'), `const fs = require('fs')
const path = require('path')
const lock = path.join(__dirname, 'build-active')
let ownsLock = false
try {
  fs.mkdirSync(lock)
  ownsLock = true
} catch (error) {
  if (error.code !== 'EEXIST') throw error
  fs.writeFileSync(path.join(__dirname, 'exceeded-task-concurrency'), '')
}
setTimeout(() => {
  if (ownsLock) fs.rmdirSync(lock)
  fs.writeFileSync(path.join(__dirname, 'ran-' + process.argv[2]), '')
}, 200)
`)

  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    tasks: {
      build: { concurrency: 1 },
    },
    workspaceConcurrency: 4,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(fs.existsSync(path.join(process.cwd(), 'exceeded-task-concurrency'))).toBe(false)
  for (const name of ['project-a', 'project-b', 'project-c']) {
    expect(fs.existsSync(path.join(process.cwd(), `ran-${name}`))).toBe(true)
  }
})

test('a project without the script is reported skipped and does not sever the chain', async () => {
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
      dependencies: {
        'project-c': 'workspace:*',
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

  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    reportSummary: true,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(server.getLines()).toStrictEqual(['project-c', 'project-a'])
  const executionStatus = readSummary()
  expect(executionStatus[path.resolve('project-a')].status).toBe('passed')
  expect(executionStatus[path.resolve('project-b')].status).toBe('skipped')
  expect(executionStatus[path.resolve('project-c')].status).toBe('passed')
})

test('without --bail, dependents of a failed task are skipped and unrelated tasks still run', async () => {
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

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
      bail: false,
      dir: process.cwd(),
      recursive: true,
      reportSummary: true,
      workspaceDir: process.cwd(),
    }, ['build'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  // The failure that blocked project-a is already counted; the skipped
  // dependent must not turn one failure into two.
  expect(err.code).toBe('ERR_PNPM_RECURSIVE_FAIL')
  expect(err.message).toContain('failed in 1 packages')
  expect(server.getLines()).toStrictEqual(['project-c'])
  const executionStatus = readSummary()
  expect(executionStatus[path.resolve('project-a')].status).toBe('skipped')
  expect(executionStatus[path.resolve('project-b')].status).toBe('failure')
  expect(executionStatus[path.resolve('project-c')].status).toBe('passed')
})

test('a workspace dependency cycle is an error naming the participating tasks', async () => {
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
      dependencies: {
        'project-a': 'workspace:*',
      },
      scripts: {
        build: server.sendLineScript('project-b'),
      },
    },
  ])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
      dir: process.cwd(),
      recursive: true,
      workspaceDir: process.cwd(),
    }, ['build'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_TASK_CYCLE')
  expect(err.message).toContain('project-a#build')
  expect(err.message).toContain('project-b#build')
  expect(server.getLines()).toStrictEqual([])
})

test('a cycle declared through dependsOn is an error', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      scripts: {
        build: 'echo build',
        test: 'echo test',
      },
    },
  ])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
      dir: process.cwd(),
      recursive: true,
      tasks: {
        build: { dependsOn: ['test'] },
        test: { dependsOn: ['build'] },
      },
      workspaceDir: process.cwd(),
    }, ['test'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_TASK_CYCLE')
})

test('--dry-run prints one stable linearization and runs nothing', async () => {
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
      dependencies: {
        'project-c': 'workspace:*',
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

  const output = await run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    dryRun: true,
    recursive: true,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect(output).toBe([
    'project-c#build',
    'project-b#build (skipped: no such script)',
    'project-a#build',
  ].join('\n'))
  expect(server.getLines()).toStrictEqual([])
})

test('--dry-run --json emits the tasks and their resolved edges', async () => {
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
        test: server.sendLineScript('project-a'),
      },
    },
    {
      name: 'project-b',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('project-b'),
      },
    },
  ])

  const output = await run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    dryRun: true,
    json: true,
    recursive: true,
    tasks: {
      build: { dependsOn: ['^build'] },
      test: { dependsOn: ['build'] },
    },
    workspaceDir: process.cwd(),
  }, ['test'])

  expect(JSON.parse(output as string)).toStrictEqual({
    tasks: [
      {
        project: 'project-a',
        script: 'build',
        missingScript: false,
        dependsOn: [{ project: 'project-b', script: 'build' }],
      },
      {
        project: 'project-a',
        script: 'test',
        missingScript: false,
        dependsOn: [{ project: 'project-a', script: 'build' }],
      },
      {
        project: 'project-b',
        script: 'build',
        missingScript: false,
        dependsOn: [],
      },
      {
        project: 'project-b',
        script: 'test',
        missingScript: true,
        dependsOn: [{ project: 'project-b', script: 'build' }],
      },
    ],
  })
  expect(server.getLines()).toStrictEqual([])
})

test('--dry-run outside a recursive run is an error', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      scripts: {
        build: 'echo build',
      },
    },
  ])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      dir: path.resolve('project-a'),
      dryRun: true,
    } as never, ['build'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }
  expect(err.code).toBe('ERR_PNPM_DRY_RUN_NOT_RECURSIVE')
})

test('ignoreWorkspaceCycles downgrades the task-cycle error to a warning', async () => {
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
      dependencies: {
        'project-a': 'workspace:*',
      },
      scripts: {
        build: server.sendLineScript('project-b'),
      },
    },
  ])

  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    ignoreWorkspaceCycles: true,
    recursive: true,
    workspaceDir: process.cwd(),
  }, ['build'])

  expect([...server.getLines()].sort()).toStrictEqual(['project-a', 'project-b'])
})

test('a missing requested script errors before upstream tasks run', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      scripts: {
        codegen: server.sendLineScript('codegen'),
      },
    },
  ])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
      dir: process.cwd(),
      recursive: true,
      tasks: {
        build: { dependsOn: ['codegen'] },
      },
      workspaceDir: process.cwd(),
    }, ['build'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  expect(err.code).toBe('ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT')
  expect(server.getLines()).toStrictEqual([])
})

test('an empty requested script matched by a RegExp errors before upstream tasks run', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      scripts: {
        'build:empty': '',
        codegen: server.sendLineScript('codegen'),
      },
    },
  ])

  await expect(run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    tasks: {
      '/^build:/': { dependsOn: ['codegen'] },
    },
    workspaceDir: process.cwd(),
  }, ['/^build:/'])).rejects.toMatchObject({
    code: 'ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT',
  })
  expect(server.getLines()).toStrictEqual([])
})

test('a RegExp selector filters hidden scripts when a visible script also matches', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      scripts: {
        'build:visible': server.sendLineScript('visible'),
        '.build:hidden': server.sendLineScript('hidden'),
      },
    },
  ])

  await run.handler({
    ...DEFAULT_OPTS,
    ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
    dir: process.cwd(),
    recursive: true,
    workspaceDir: process.cwd(),
  }, ['/build/'])

  expect(server.getLines()).toStrictEqual(['visible'])
})

test('a failed upstream task is reported as the failure, not as a missing script', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      scripts: {
        build: 'exit 1',
        test: 'echo test',
      },
    },
  ])

  let err!: PnpmError
  try {
    await run.handler({
      ...DEFAULT_OPTS,
      ...await filterProjectsBySelectorObjectsFromDir(process.cwd(), []),
      bail: false,
      dir: process.cwd(),
      recursive: true,
      reportSummary: true,
      tasks: {
        test: { dependsOn: ['build'] },
      },
      workspaceDir: process.cwd(),
    }, ['test'])
  } catch (_err: any) { // eslint-disable-line
    err = _err
  }

  // Every requested task was skipped because its build dependency failed;
  // the run must report that failure, not RECURSIVE_RUN_NO_SCRIPT.
  expect(err.code).toBe('ERR_PNPM_RECURSIVE_FAIL')
  expect(err.message).toContain('failed in 1 packages')
  const executionStatus = readSummary()
  expect(executionStatus[path.resolve('project-a')].status).toBe('skipped')
  expect(executionStatus[`${path.resolve('project-a')}#build`].status).toBe('failure')
})

test('the tolerated cycle warning reaches the CLI output', async () => {
  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      dependencies: {
        'project-b': 'workspace:*',
      },
      scripts: {
        build: 'echo project-a',
      },
    },
    {
      name: 'project-b',
      version: '1.0.0',
      dependencies: {
        'project-a': 'workspace:*',
      },
      scripts: {
        build: 'echo project-b',
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**'],
    ignoreWorkspaceCycles: true,
  })

  const { stdout } = await execa(pnpmBin, ['run', '-r', 'build'])
  expect(stdout).toContain('The tasks form a dependency cycle')
})

test('the tasks section of pnpm-workspace.yaml reaches the CLI run', async () => {
  await using server = await createTestIpcServer()

  preparePackages([
    {
      name: 'project-a',
      version: '1.0.0',
      dependencies: {
        'project-b': 'workspace:*',
      },
      scripts: {
        build: server.sendLineScript('a-build'),
        test: server.sendLineScript('a-test'),
      },
    },
    {
      name: 'project-b',
      version: '1.0.0',
      scripts: {
        build: server.sendLineScript('b-build'),
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['**'],
    tasks: {
      build: { dependsOn: ['^build'] },
      test: { dependsOn: ['build'] },
    },
  })

  await execa(pnpmBin, ['run', '-r', 'test'])

  const order = server.getLines()
  expect([...order].sort()).toStrictEqual(['a-build', 'a-test', 'b-build'])
  expect(order.indexOf('b-build')).toBeLessThan(order.indexOf('a-build'))
  expect(order.indexOf('a-build')).toBeLessThan(order.indexOf('a-test'))
})

function readSummary (): Record<string, { status: string }> {
  return JSON.parse(fs.readFileSync('pnpm-exec-summary.json', 'utf8')).executionStatus
}
