import { type ChildProcess, spawn } from 'node:child_process'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { killTrackedProcessTrees, trackChildProcess } from '@pnpm/exec.lifecycle'
import isWindows from 'is-windows'

const parentScript = path.join(import.meta.dirname, 'fixtures/process-tree/parent.cjs')

test('killTrackedProcessTrees() kills a tracked child process and, on Windows, its descendants', async () => {
  const child = spawn(process.execPath, [parentScript], { stdio: ['ignore', 'pipe', 'ignore'] })
  const childExited = new Promise<void>((resolve) => {
    child.once('exit', () => {
      resolve()
    })
  })
  const grandchildPid = await readGrandchildPid(child)
  try {
    trackChildProcess(child)
    await killTrackedProcessTrees()

    await childExited
    expect(child.exitCode !== 0 || child.signalCode != null).toBe(true)
    if (isWindows()) {
      expect(await exited(grandchildPid)).toBe(true)
    }
  } finally {
    killSilently(child.pid!)
    killSilently(grandchildPid)
  }
})

test('killTrackedProcessTrees() is a no-op for a tracked child process that already exited', async () => {
  const child = spawn(process.execPath, ['-e', ''], { stdio: 'ignore' })
  trackChildProcess(child)
  await new Promise<void>((resolve) => {
    child.once('close', () => {
      resolve()
    })
  })
  await killTrackedProcessTrees()
})

async function readGrandchildPid (child: ChildProcess): Promise<number> {
  const firstLine = await new Promise<string>((resolve, reject) => {
    let output = ''
    child.stdout!.on('data', (data: Buffer) => {
      output += data.toString()
      if (output.includes('\n')) {
        resolve(output)
      }
    })
    child.once('error', reject)
    child.once('exit', () => {
      reject(new Error(`the spawned process tree exited early: ${output}`))
    })
  })
  return parseInt(firstLine, 10)
}

async function exited (pid: number, timeoutMs: number = 10_000): Promise<boolean> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      process.kill(pid, 0)
    } catch {
      return true
    }
    await new Promise<void>((resolve) => setTimeout(resolve, 100))
  }
  return false
}

function killSilently (pid: number): void {
  try {
    process.kill(pid, 'SIGKILL')
  } catch {
    // the process has already exited
  }
}
