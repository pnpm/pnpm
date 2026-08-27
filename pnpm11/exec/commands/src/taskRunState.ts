import crypto from 'node:crypto'
import fs, { type FileHandle } from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { createHexHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import type { TaskGraph, TaskKey, TaskNode } from '@pnpm/workspace.task-scheduler'
import writeFileAtomic from 'write-file-atomic'

const STATE_VERSION = 1
const STATE_DIR = '.pnpm-task-run-state-v1'
const LATEST_STATE_FILE = 'latest.json'
const RUN_ID = /^[0-9a-f-]{1,128}$/

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

export interface TaskRunExecutionSettings {
  extraBinPaths?: string[]
  extraEnv?: Record<string, string | undefined>
  modulesDir?: string
  nodeExperimentalPackageMap?: boolean
  nodeOptions?: string
  userAgent?: string
}

export function taskRunExecutionSettings (opts: TaskRunExecutionSettings): string[] {
  const extraEnv = Object.entries(opts.extraEnv ?? {})
    .sort(([left], [right]) => compareStrings(left, right))
    .map(([key, value]) => [key, value ?? null])
  return [
    `extra-bin-paths=${JSON.stringify(opts.extraBinPaths ?? [])}`,
    `extra-env=${JSON.stringify(extraEnv)}`,
    `modules-dir=${opts.modulesDir ?? 'node_modules'}`,
    `node-experimental-package-map=${Boolean(opts.nodeExperimentalPackageMap)}`,
    `node-options=${opts.nodeOptions ?? ''}`,
    `user-agent=${opts.userAgent ?? ''}`,
  ]
}

export class TaskRunStateContext {
  readonly invocation: string
  readonly latestStatePath: string
  private readonly opts: TaskRunStateContextOptions
  private readonly nodeModulesDir: string
  private readonly stateDir: string
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
    this.nodeModulesDir = path.join(opts.workspaceDir, 'node_modules')
    this.stateDir = path.join(this.nodeModulesDir, STATE_DIR)
    this.latestStatePath = path.join(this.stateDir, LATEST_STATE_FILE)
  }

  async readCompletedTasks (): Promise<Set<TaskKey> | undefined> {
    if (!await this.validateStateDirectory(false)) return undefined
    let latest: StateHeader
    try {
      latest = JSON.parse(await fs.readFile(this.latestStatePath, 'utf8')) as StateHeader
    } catch (err: unknown) {
      if (util.types.isNativeError(err) && 'code' in err && err.code !== 'ENOENT') throw err
      return undefined
    }
    if (latest.version !== STATE_VERSION || latest.invocation !== this.invocation || !RUN_ID.test(latest.run)) return undefined
    const filePath = this.journalPath(latest.run)
    let contents: string
    try {
      contents = await fs.readFile(filePath, 'utf8')
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
    if (header.version !== STATE_VERSION || header.invocation !== this.invocation || header.run !== latest.run) return undefined
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
    const filePath = this.journalPath(run)
    await this.validateStateDirectory(true)
    await writeFileAtomic(filePath, contents, { mode: 0o600 })
    const file = await fs.open(filePath, 'a')
    try {
      await writeFileAtomic(this.latestStatePath, JSON.stringify(header), { mode: 0o600 })
    } catch (err: unknown) {
      await file.close()
      await unlinkIfExists(filePath)
      throw err
    }
    return new TaskRunState(filePath, file, this.opts.workspaceDir, run, completedTasks)
  }

  private journalPath (run: string): string {
    return path.join(this.stateDir, `${this.invocation}.${run}.jsonl`)
  }

  private async validateStateDirectory (create: boolean): Promise<boolean> {
    if (!await validateRealDirectory(this.nodeModulesDir, create)) return false
    return validateRealDirectory(this.stateDir, create)
  }
}

export class TaskRunState {
  readonly filePath: string
  private readonly file: FileHandle
  private readonly workspaceDir: string
  private readonly run: string
  private readonly completedTasks: Set<TaskKey>
  private pendingWrite: Promise<void> = Promise.resolve()
  private closePromise: Promise<void> | undefined

  constructor (
    filePath: string,
    file: FileHandle,
    workspaceDir: string,
    run: string,
    completedTasks: ReadonlySet<TaskKey>
  ) {
    this.filePath = filePath
    this.file = file
    this.workspaceDir = workspaceDir
    this.run = run
    this.completedTasks = new Set(completedTasks)
  }

  async recordPassed (key: TaskKey, node: TaskNode): Promise<void> {
    if (this.completedTasks.has(key)) return
    this.completedTasks.add(key)
    const record: TaskRecord = { run: this.run, ...taskId(node, this.workspaceDir) }
    const line = `${JSON.stringify(record)}\n`
    const write = this.pendingWrite.then(async () => {
      await this.file.appendFile(line)
    })
    this.pendingWrite = write.catch(() => {})
    try {
      await write
    } catch (err: unknown) {
      this.completedTasks.delete(key)
      throw err
    }
  }

  async finish (): Promise<void> {
    await this.close()
    await unlinkIfExists(this.filePath)
  }

  async close (): Promise<void> {
    this.closePromise ??= this.pendingWrite.then(async () => this.file.close())
    await this.closePromise
  }
}

async function validateRealDirectory (dir: string, create: boolean): Promise<boolean> {
  let stats
  try {
    stats = await fs.lstat(dir)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) throw err
    if (!create) return false
    try {
      await fs.mkdir(dir)
    } catch (mkdirErr: unknown) {
      if (!(util.types.isNativeError(mkdirErr) && 'code' in mkdirErr && mkdirErr.code === 'EEXIST')) throw mkdirErr
    }
    stats = await fs.lstat(dir)
  }
  if (stats.isSymbolicLink() || !stats.isDirectory()) {
    throw new PnpmError('UNSAFE_TASK_RUN_STATE_PATH', `Refusing to use task run state directory at "${dir}" because it is a symbolic link or not a directory`)
  }
  return true
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
