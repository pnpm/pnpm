import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'
import { type TaskGraph, taskKey } from '@pnpm/workspace.task-scheduler'

import { taskRunExecutionSettings, TaskRunStateContext } from '../src/taskRunState.js'

const temporaryDirectories: string[] = []
const testOnPosix = process.platform === 'win32' ? test.skip : test

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map(async (dir) => fs.rm(dir, { force: true, recursive: true })))
})

test('task execution settings have a stable canonical encoding', () => {
  expect(taskRunExecutionSettings({
    extraBinPaths: ['tools', 'other-tools'],
    extraEnv: { ZED: 'last', ALPHA: 'first' },
    modulesDir: 'vendor',
    nodeExperimentalPackageMap: true,
    nodeOptions: '--conditions=development',
    userAgent: 'pnpm/test',
  })).toStrictEqual([
    'extra-bin-paths=["tools","other-tools"]',
    'extra-env=[["ALPHA","first"],["ZED","last"]]',
    'modules-dir=vendor',
    'node-experimental-package-map=true',
    'node-options=--conditions=development',
    'user-agent=pnpm/test',
  ])
})

test('a changed execution setting produces a different invocation identity', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const contextOptions = {
    command: 'run' as const,
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  }
  const first = new TaskRunStateContext({
    ...contextOptions,
    settings: taskRunExecutionSettings({ extraEnv: { MODE: 'first' } }),
  })
  const second = new TaskRunStateContext({
    ...contextOptions,
    settings: taskRunExecutionSettings({ extraEnv: { MODE: 'second' } }),
  })

  expect(first.invocation).not.toBe(second.invocation)
})

test('task run state ignores a torn trailing record and removes a completed journal', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const firstProject = path.join(workspaceDir, 'first') as ProjectRootDir
  const secondProject = path.join(workspaceDir, 'second') as ProjectRootDir
  const firstKey = taskKey(firstProject, 'build')
  const secondKey = taskKey(secondProject, 'build')
  const graph: TaskGraph = new Map([
    [firstKey, {
      project: firstProject,
      taskName: 'build',
      scripts: ['build'],
      requested: true,
      dependencies: [],
    }],
    [secondKey, {
      project: secondProject,
      taskName: 'build',
      scripts: ['build'],
      requested: true,
      dependencies: [firstKey],
    }],
  ])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  expect(context.invocation).toBe('76a575bfcc3b67becd98b5ec661be54567b53954a723da167603dad119fab140')
  const state = await context.start(new Set([firstKey]))
  await state.recordPassed(secondKey, graph.get(secondKey)!)
  await fs.appendFile(state.filePath, '{"run":"superseded","project":"unknown","task":"build"}\n')
  await fs.appendFile(state.filePath, '{"project":"torn')

  await expect(context.readCompletedTasks()).resolves.toStrictEqual(new Set([firstKey, secondKey]))
  await state.finish()
  await expect(context.readCompletedTasks()).resolves.toBeUndefined()
})

test('task run state rejects a malformed complete record', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  const state = await context.start(new Set())
  await fs.appendFile(state.filePath, 'not-json\n')

  await expect(context.readCompletedTasks()).resolves.toBeUndefined()
  await state.close()
})

test('task run state recovers its write queue after an append failure', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  const state = await context.start(new Set())
  const file = (state as unknown as { file: { appendFile: (data: string) => Promise<void> } }).file
  const appendFile = jest.spyOn(file, 'appendFile').mockRejectedValueOnce(new Error('append failed'))

  await expect(state.recordPassed(key, graph.get(key)!)).rejects.toThrow()

  await state.recordPassed(key, graph.get(key)!)
  appendFile.mockRestore()
  await expect(context.readCompletedTasks()).resolves.toStrictEqual(new Set([key]))
  await state.finish()
})

test('task run state is disabled after an append permission error', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const secondKey = taskKey(project, 'test')
  const graph: TaskGraph = new Map([
    [key, {
      project,
      taskName: 'build',
      scripts: ['build'],
      requested: true,
      dependencies: [],
    }],
    [secondKey, {
      project,
      taskName: 'test',
      scripts: ['test'],
      requested: true,
      dependencies: [],
    }],
  ])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  const state = await context.start(new Set())
  const file = (state as unknown as { file: { appendFile: (data: string) => Promise<void> } }).file
  const appendFile = jest.spyOn(file, 'appendFile').mockRejectedValueOnce(Object.assign(new Error('permission denied'), { code: 'EACCES' }))

  await state.recordPassed(key, graph.get(key)!)
  await state.recordPassed(secondKey, graph.get(secondKey)!)

  expect(appendFile).toHaveBeenCalledTimes(1)
  await expect(fs.access(state.filePath)).rejects.toMatchObject({ code: 'ENOENT' })
  await expect(context.readCompletedTasks()).resolves.toBeUndefined()
  appendFile.mockRestore()
  await state.finish()
})

test('task run state stops waiting when an abandoned lock cannot be removed', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  const lockPath = path.join(workspaceDir, 'node_modules', '.pnpm-task-run-state-v1', 'start.lock')
  await fs.mkdir(lockPath, { recursive: true })
  const now = Date.now()
  const staleTime = new Date(now - 60_000)
  await fs.utimes(lockPath, staleTime, staleTime)
  const rm = jest.spyOn(fs, 'rm').mockRejectedValue(Object.assign(new Error('permission denied'), { code: 'EACCES' }))
  const dateNow = jest.spyOn(Date, 'now')
    .mockReturnValueOnce(now)
    .mockReturnValueOnce(now)
    .mockReturnValue(now + 2_000)

  try {
    const state = await context.start(new Set())

    expect(rm).toHaveBeenCalledTimes(1)
    await expect(fs.access(context.latestStatePath)).rejects.toMatchObject({ code: 'ENOENT' })
    await state.finish()
  } finally {
    dateNow.mockRestore()
    rm.mockRestore()
  }
})

test('task run state rejects a symlinked state directory', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  const outsideDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-outside-'))
  temporaryDirectories.push(workspaceDir, outsideDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const nodeModulesDir = path.join(workspaceDir, 'node_modules')
  await fs.mkdir(nodeModulesDir)
  await fs.symlink(outsideDir, path.join(nodeModulesDir, '.pnpm-task-run-state-v1'), process.platform === 'win32' ? 'junction' : 'dir')
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })

  await expect(context.start(new Set())).rejects.toMatchObject({ code: 'ERR_PNPM_UNSAFE_TASK_RUN_STATE_PATH' })
  await expect(fs.access(path.join(outsideDir, 'latest.json'))).rejects.toMatchObject({ code: 'ENOENT' })
})

testOnPosix('task run state is disabled when node_modules is read-only', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const nodeModulesDir = path.join(workspaceDir, 'node_modules')
  await fs.mkdir(nodeModulesDir)
  await fs.chmod(nodeModulesDir, 0o555)
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })

  try {
    const state = await context.start(new Set())
    await state.recordPassed(key, graph.get(key)!)
    await state.finish()
    await expect(fs.access(context.latestStatePath)).rejects.toMatchObject({ code: 'ENOENT' })
  } finally {
    await fs.chmod(nodeModulesDir, 0o755)
  }
})

test('finishing an older invocation preserves the newer invocation journal', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  const older = await context.start(new Set())
  const newer = await context.start(new Set([key]))

  await older.finish()

  await expect(context.readCompletedTasks()).resolves.toStrictEqual(new Set([key]))
  await newer.finish()
  await expect(context.readCompletedTasks()).resolves.toBeUndefined()
})

test('a finished journal is not resumable when cleanup is unavailable', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  const state = await context.start(new Set([key]))
  const unlink = jest.spyOn(fs, 'unlink').mockRejectedValue(Object.assign(new Error('permission denied'), { code: 'EACCES' }))

  try {
    await state.finish()
  } finally {
    unlink.mockRestore()
  }

  await expect(context.readCompletedTasks()).resolves.toBeUndefined()
})

test('a stale pointer does not hide a newer published invocation journal', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  const older = await context.start(new Set())
  const olderHeader = (await fs.readFile(older.filePath, 'utf8')).split('\n')[0]
  const newer = await context.start(new Set([key]))
  const newerPublishedPath = newer.filePath.replace(/\.jsonl$/, '.published')

  await fs.unlink(newerPublishedPath)
  await fs.writeFile(context.latestStatePath, olderHeader)
  await expect(context.readCompletedTasks()).resolves.toStrictEqual(new Set())
  await fs.writeFile(newerPublishedPath, '')

  await expect(context.readCompletedTasks()).resolves.toStrictEqual(new Set([key]))
  await older.finish()
  await expect(context.readCompletedTasks()).resolves.toStrictEqual(new Set([key]))
  await newer.finish()
})

test('a stale start does not overwrite a newer invocation', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  const originalOpen = fs.open.bind(fs)
  let markOlderBlocked!: () => void
  let unblockOlder!: () => void
  const olderBlocked = new Promise<void>(resolve => {
    markOlderBlocked = resolve
  })
  const olderGate = new Promise<void>(resolve => {
    unblockOlder = resolve
  })
  let blockNextJournal = true
  const open = jest.spyOn(fs, 'open').mockImplementation(async (...args) => {
    if (blockNextJournal && String(args[0]).endsWith('.jsonl')) {
      blockNextJournal = false
      markOlderBlocked()
      await olderGate
    }
    return originalOpen(...args)
  })

  try {
    const olderPromise = context.start(new Set())
    await olderBlocked
    const lockPath = path.join(workspaceDir, 'node_modules', '.pnpm-task-run-state-v1', 'start.lock')
    const staleTime = new Date(Date.now() - 60_000)
    await fs.utimes(lockPath, staleTime, staleTime)
    const newer = await context.start(new Set([key]))
    unblockOlder()
    const older = await olderPromise

    await expect(context.readCompletedTasks()).resolves.toStrictEqual(new Set([key]))
    await older.close()
    await newer.close()
  } finally {
    unblockOlder()
    open.mockRestore()
  }
})

test('a stale start cannot revive state after a newer invocation finishes', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-task-state-'))
  temporaryDirectories.push(workspaceDir)
  const project = path.join(workspaceDir, 'project') as ProjectRootDir
  const key = taskKey(project, 'build')
  const graph: TaskGraph = new Map([[key, {
    project,
    taskName: 'build',
    scripts: ['build'],
    requested: true,
    dependencies: [],
  }]])
  const context = new TaskRunStateContext({
    command: 'run',
    params: ['build'],
    graph,
    workspaceDir,
    scriptCommands: () => ['build-command'],
  })
  const older = await context.start(new Set())
  const olderContents = await fs.readFile(older.filePath, 'utf8')
  const olderPublishedPath = older.filePath.replace(/\.jsonl$/, '.published')
  const newer = await context.start(new Set([key]))

  await newer.finish()
  await fs.writeFile(older.filePath, olderContents)
  await fs.writeFile(olderPublishedPath, '')
  await fs.writeFile(context.latestStatePath, olderContents.split('\n')[0])

  await expect(context.readCompletedTasks()).resolves.toBeUndefined()
  await older.close()
})
