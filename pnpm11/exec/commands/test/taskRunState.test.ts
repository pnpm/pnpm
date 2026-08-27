import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'
import { type TaskGraph, taskKey } from '@pnpm/workspace.task-scheduler'

import { TaskRunStateContext } from '../src/taskRunState.js'

const temporaryDirectories: string[] = []

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map(async (dir) => fs.rm(dir, { force: true, recursive: true })))
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
  const staleState = path.join(path.dirname(context.filePath), `${'a'.repeat(64)}.jsonl`)
  await fs.mkdir(path.dirname(staleState), { recursive: true })
  await fs.writeFile(staleState, 'stale')
  const state = await context.start(new Set([firstKey]))
  await expect(fs.stat(staleState)).rejects.toMatchObject({ code: 'ENOENT' })
  await state.recordPassed(secondKey, graph.get(secondKey)!)
  await fs.appendFile(context.filePath, '{"run":"superseded","project":"unknown","task":"build"}\n')
  await fs.appendFile(context.filePath, '{"project":"torn')

  await expect(context.readCompletedTasks()).resolves.toStrictEqual(new Set([firstKey, secondKey]))
  await state.finish()
  await expect(fs.stat(context.filePath)).rejects.toMatchObject({ code: 'ENOENT' })
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
  await context.start(new Set())
  await fs.appendFile(context.filePath, 'not-json\n')

  await expect(context.readCompletedTasks()).resolves.toBeUndefined()
})
