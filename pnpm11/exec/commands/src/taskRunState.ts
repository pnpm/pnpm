import crypto from 'node:crypto'
import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { createHexHash } from '@pnpm/crypto.hash'
import type { TaskGraph, TaskKey, TaskNode } from '@pnpm/workspace.task-scheduler'
import writeFileAtomic from 'write-file-atomic'

const STATE_VERSION = 1
const STATE_DIR = '.pnpm-task-run-state-v1'
const STATE_FILE_NAME = /^[0-9a-f]{64}\.jsonl$/

interface TaskId {
  project: string
  task: string
}

interface TaskIdentity extends TaskId {
  scripts: Array<{ name: string, commands: string[] }>
  requested: boolean
  dependencies: TaskId[]
}

interface InvocationIdentity {
  command: 'run' | 'exec'
  params: string[]
  settings: string[]
  tasks: TaskIdentity[]
}

interface StateHeader {
  version: number
  invocation: string
  run: string
}

interface TaskRecord extends TaskId {
  run: string
}

export interface TaskRunStateContextOptions {
  command: InvocationIdentity['command']
  params: string[]
  settings?: string[]
  graph: TaskGraph
  workspaceDir: string
  scriptCommands: (node: TaskNode, script: string) => string[]
}

export class TaskRunStateContext {
  readonly filePath: string
  readonly invocation: string
  private readonly opts: TaskRunStateContextOptions
  private readonly keysById = new Map<string, TaskKey>()

  constructor (opts: TaskRunStateContextOptions) {
    this.opts = opts
    const identity: InvocationIdentity = {
      command: opts.command,
      params: opts.params,
      settings: [...(opts.settings ?? [])].sort(compareStrings),
      tasks: [...opts.graph].map(([key, node]) => {
        const id = taskId(node, opts.workspaceDir)
        this.keysById.set(taskIdKey(id), key)
        return {
          ...id,
          scripts: node.scripts
            .map((name) => ({ name, commands: opts.scriptCommands(node, name) }))
            .sort(compareScripts),
          requested: node.requested,
          dependencies: node.dependencies
            .map((dependency) => taskId(opts.graph.get(dependency)!, opts.workspaceDir))
            .sort(compareTaskIds),
        }
      }).sort(compareTaskIds),
    }
    this.invocation = createHexHash(JSON.stringify(identity))
    this.filePath = path.join(opts.workspaceDir, 'node_modules', STATE_DIR, `${this.invocation}.jsonl`)
  }

  async readCompletedTasks (): Promise<Set<TaskKey> | undefined> {
    let contents: string
    try {
      contents = await fs.readFile(this.filePath, 'utf8')
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return undefined
      throw err
    }
    // A record is committed by its newline; a process killed during append
    // can leave only the final record torn.
    const lines = contents.split('\n')
    lines.pop()
    if (lines.length === 0) return undefined
    let header: StateHeader
    try {
      header = JSON.parse(lines[0]) as StateHeader
    } catch {
      return undefined
    }
    if (header.version !== STATE_VERSION || header.invocation !== this.invocation) return undefined
    const completed = new Set<TaskKey>()
    for (const line of lines.slice(1)) {
      let record: TaskRecord
      try {
        record = JSON.parse(line) as TaskRecord
      } catch {
        return undefined
      }
      if (record.run !== header.run) continue
      const key = this.keysById.get(taskIdKey(record))
      if (key == null) return undefined
      completed.add(key)
    }
    return completed
  }

  async start (completedTasks: ReadonlySet<TaskKey>): Promise<TaskRunState> {
    const run = crypto.randomUUID()
    const header: StateHeader = { version: STATE_VERSION, invocation: this.invocation, run }
    const completed = [...completedTasks]
      .map((key): TaskRecord => ({ run, ...taskId(this.opts.graph.get(key)!, this.opts.workspaceDir) }))
      .sort(compareTaskIds)
    const contents = [header, ...completed].map((record) => JSON.stringify(record)).join('\n') + '\n'
    const stateDir = path.dirname(this.filePath)
    await fs.mkdir(stateDir, { recursive: true })
    await writeFileAtomic(this.filePath, contents, { mode: 0o600 })
    // Only the latest recursive invocation is resumable. Otherwise an old
    // compatible journal could become active again after intervening work.
    const entries = await fs.readdir(stateDir, { withFileTypes: true })
    await Promise.all(entries
      .filter((entry) => entry.name !== path.basename(this.filePath) && !entry.isDirectory() && STATE_FILE_NAME.test(entry.name))
      .map(async (entry) => unlinkIfExists(path.join(stateDir, entry.name))))
    return new TaskRunState(this.filePath, this.opts.workspaceDir, run, completedTasks)
  }
}

export class TaskRunState {
  private readonly filePath: string
  private readonly workspaceDir: string
  private readonly run: string
  private readonly completedTasks: Set<TaskKey>
  private pendingWrite: Promise<void> = Promise.resolve()

  constructor (
    filePath: string,
    workspaceDir: string,
    run: string,
    completedTasks: ReadonlySet<TaskKey>
  ) {
    this.filePath = filePath
    this.workspaceDir = workspaceDir
    this.run = run
    this.completedTasks = new Set(completedTasks)
  }

  async recordPassed (key: TaskKey, node: TaskNode): Promise<void> {
    if (this.completedTasks.has(key)) return
    this.completedTasks.add(key)
    const record: TaskRecord = { run: this.run, ...taskId(node, this.workspaceDir) }
    const line = `${JSON.stringify(record)}\n`
    this.pendingWrite = this.pendingWrite.then(async () => {
      await fs.appendFile(this.filePath, line)
    })
    await this.pendingWrite
  }

  async finish (): Promise<void> {
    await this.pendingWrite
    await unlinkIfExists(this.filePath)
  }
}

function taskId (node: TaskNode, workspaceDir: string): TaskId {
  const relative = path.relative(workspaceDir, node.project)
  return {
    project: relative === '' ? '.' : relative.replaceAll(path.sep, '/'),
    task: node.taskName,
  }
}

function taskIdKey (id: TaskId): string {
  return `${id.project}\0${id.task}`
}

function compareTaskIds (left: TaskId, right: TaskId): number {
  return compareStrings(left.project, right.project) || compareStrings(left.task, right.task)
}

function compareScripts (left: { name: string }, right: { name: string }): number {
  return compareStrings(left.name, right.name)
}

function compareStrings (left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0
}

async function unlinkIfExists (filePath: string): Promise<void> {
  try {
    await fs.unlink(filePath)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) throw err
  }
}
