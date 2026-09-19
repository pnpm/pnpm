import { spawn } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { killProcessGroup } from '@pnpm/prepare'
import { temporaryDirectory } from 'tempy'

import { waitForProcessGroup } from '../src/signals.js'

const testOnLinux = process.platform === 'linux' ? test : test.skip

testOnLinux('the wait ends once the group holds only a zombie, whatever else the process table shows', async () => {
  // A real group, so the kernel still counts a member of it.
  const child = spawn('sleep', ['30'], { detached: true, stdio: 'ignore' })
  const group = child.pid!
  try {
    const table = writeProcessTable([
      { pid: group, state: 'Z', group },
      // Another user's entry under a restricted process table: listed, but
      // its state cannot be read, so it cannot be a member pnpm observes.
      { pid: 999999, state: 'S', group, readable: false },
    ])
    expect(await withDeadline(waitForProcessGroup(group, { processTable: table }), 5_000)).toBeUndefined()
  } finally {
    killProcessGroup(group)
  }
})

testOnLinux('the wait goes on while the group holds a live member', async () => {
  const child = spawn('sleep', ['30'], { detached: true, stdio: 'ignore' })
  const group = child.pid!
  try {
    const table = writeProcessTable([{ pid: group, state: 'S', group }])
    expect(await withDeadline(waitForProcessGroup(group, { processTable: table }), 500)).toBe('timed out')
  } finally {
    killProcessGroup(group)
  }
})

testOnLinux('the wait ends once the kernel no longer knows the group', async () => {
  const child = spawn('sleep', ['30'], { detached: true, stdio: 'ignore' })
  const group = child.pid!
  const exited = new Promise<void>((resolve) => {
    child.on('exit', () => {
      resolve()
    })
  })
  killProcessGroup(group)
  await exited
  expect(await withDeadline(waitForProcessGroup(group), 5_000)).toBeUndefined()
})

/** A `stat` line is the kernel's: pid, command in parentheses, state, parent, process group. */
function writeProcessTable (entries: Array<{ pid: number, state: string, group: number, readable?: boolean }>): string {
  const table = temporaryDirectory()
  for (const { pid, state, group, readable = true } of entries) {
    fs.mkdirSync(path.join(table, String(pid)))
    fs.writeFileSync(path.join(table, String(pid), 'stat'), `${pid} (node) ${state} 1 ${group} ${group}\n`, { mode: readable ? 0o644 : 0o000 })
  }
  return table
}

async function withDeadline<T> (promise: Promise<T>, timeout: number): Promise<T | 'timed out'> {
  let timer: NodeJS.Timeout | undefined
  const deadline = new Promise<'timed out'>((resolve) => {
    timer = setTimeout(() => resolve('timed out'), timeout)
  })
  try {
    return await Promise.race([promise, deadline])
  } finally {
    clearTimeout(timer)
  }
}
