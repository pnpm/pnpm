import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'
import { type TaskGraph, taskKey } from '@pnpm/workspace.task-scheduler'

import { taskRunExecutionSettings, TaskRunStateContext } from '../src/taskRunState.js'

const temporaryDirectories: string[] = []

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
