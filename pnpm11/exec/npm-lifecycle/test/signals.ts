import { type ChildProcess, spawn, spawnSync } from 'node:child_process'
import { once } from 'node:events'
import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { endsWithin, killProcessGroup } from '@pnpm/prepare'
import { temporaryDirectory } from 'tempy'
import which from 'which'

import { relaySignals, reserveSignalRelay, waitForProcessGroup, watchProcessGroup } from '../src/signals.js'

const testOnLinux = process.platform === 'linux' ? test : test.skip
const testOnPosix = process.platform === 'win32' ? test.skip : test
const bash = process.platform === 'win32' ? null : which.sync('bash', { nothrow: true })
const testWithBash = bash == null ? test.skip : test
const watchScript = path.join(import.meta.dirname, 'fixtures', 'watchdog', 'watch.mjs')
const startupScript = path.join(import.meta.dirname, 'fixtures', 'watchdog', 'startup.mjs')

testWithBash.each([
  { scriptShell: bash!, exportedOptions: false },
  { scriptShell: 'bash', exportedOptions: false },
  { scriptShell: './bash', exportedOptions: false },
  { scriptShell: bash!, exportedOptions: true },
  { scriptShell: bash!, exportedOptions: false, withoutSh: true },
])('a $scriptShell script keeps exported functions and loads BASH_ENV once (options: $exportedOptions, without sh: $withoutSh)', ({ scriptShell, exportedOptions, withoutSh = false }) => {
  const directory = temporaryDirectory()
  const startupFile = path.join(directory, 'bash-env')
  const startupMarker = path.join(directory, 'loaded')
  const functionMarker = path.join(directory, 'greeted')
  try {
    if (!withoutSh) fs.symlinkSync(which.sync('dash', { nothrow: true }) ?? '/bin/sh', path.join(directory, 'sh'))
    fs.symlinkSync(bash!, path.join(directory, 'bash'))
    fs.writeFileSync(startupFile, 'printf "loaded\\n" >> "$BASH_ENV_MARKER"\n')
    const result = spawnSync(process.execPath, [path.join(import.meta.dirname, 'fixtures', 'watchdog', 'bash.mjs'), directory, scriptShell, String(exportedOptions)], {
      cwd: directory,
      env: {
        ...process.env,
        PATH: withoutSh ? directory : `${directory}${path.delimiter}${process.env.PATH ?? '/usr/bin:/bin'}`,
        BASH_ENV: startupFile,
        BASH_ENV_MARKER: startupMarker,
        FUNCTION_MARKER: functionMarker,
        'BASH_FUNC_greet%%': '() { printf greeted > "$FUNCTION_MARKER"; }',
        ...(exportedOptions ? { SHELLOPTS: 'pipefail', BASHOPTS: 'nullglob' } : {}),
      },
      encoding: 'utf8',
      timeout: 10_000,
    })
    expect({ status: result.status, stderr: result.stderr }).toStrictEqual({ status: 0, stderr: '' })
    expect(fs.readFileSync(functionMarker, 'utf8')).toBe('greeted')
    expect(fs.readFileSync(startupMarker, 'utf8')).toBe('loaded\n')
  } finally {
    fs.rmSync(directory, { recursive: true, force: true })
  }
}, 15_000)

testWithBash('a script run by a shell other than sh keeps environment variables that are not identifiers', () => {
  const directory = temporaryDirectory()
  try {
    fs.symlinkSync(which.sync('dash', { nothrow: true }) ?? '/bin/sh', path.join(directory, 'sh'))
    fs.symlinkSync(bash!, path.join(directory, 'other-shell'))
    const result = spawnSync(process.execPath, [path.join(import.meta.dirname, 'fixtures', 'watchdog', 'bash.mjs'), directory, path.join(directory, 'other-shell'), 'non-identifier'], {
      cwd: directory,
      env: {
        ...process.env,
        PATH: `${directory}${path.delimiter}${process.env.PATH ?? '/usr/bin:/bin'}`,
        'NON-IDENTIFIER': 'kept',
      },
      encoding: 'utf8',
      timeout: 10_000,
    })
    expect({ status: result.status, stderr: result.stderr }).toStrictEqual({ status: 0, stderr: '' })
  } finally {
    fs.rmSync(directory, { recursive: true, force: true })
  }
}, 15_000)

testOnPosix('a script runs when the watchdog cannot be started', () => {
  const directory = temporaryDirectory()
  try {
    const result = spawnSync(process.execPath, [path.join(import.meta.dirname, 'fixtures', 'watchdog', 'unavailable.mjs'), directory, 'runner'], {
      encoding: 'utf8',
      timeout: 10_000,
    })
    expect({ status: result.status, stderr: result.stderr }).toStrictEqual({ status: 0, stderr: '' })
    expect(fs.readFileSync(path.join(directory, 'ran'), 'utf8')).toBe('ran')
  } finally {
    fs.rmSync(directory, { recursive: true, force: true })
  }
}, 15_000)

testOnPosix.each(['observed', 'pending', 'destroyed'])('a script starts after its inherited watchdog pipe loses its reader (%s replacement)', (replacement) => {
  const directory = temporaryDirectory()
  try {
    const result = spawnSync(process.execPath, [path.join(import.meta.dirname, 'fixtures', 'watchdog', 'restart.mjs'), directory, 'runner', replacement], {
      encoding: 'utf8',
      timeout: 10_000,
    })
    expect({ status: result.status, stderr: result.stderr }).toStrictEqual({ status: 0, stderr: '' })
    expect(fs.readFileSync(path.join(directory, 'ran'), 'utf8')).toBe('ran')
  } finally {
    fs.rmSync(directory, { recursive: true, force: true })
  }
}, 15_000)

testOnPosix.each([
  { count: 1, mode: 'default' },
  { count: 3, mode: 'default' },
  { count: 1, mode: 'relative' },
  { count: 1, mode: 'relative-path' },
  { count: 1, mode: 'extended' },
  ...bash == null ? [] : [{ count: 1, mode: 'bash' }, { count: 1, mode: 'exported-printf' }],
])('$count $mode scripts and their descendants are watched before the spawn returns', async ({ count, mode }) => {
  const directory = temporaryDirectory()
  if (mode === 'relative') fs.symlinkSync('/bin/sh', path.join(directory, 'script-shell'))
  if (mode === 'relative-path') {
    fs.mkdirSync(path.join(directory, 'runner', 'bin'), { recursive: true })
    fs.symlinkSync('/bin/sh', path.join(directory, 'runner', 'bin', 'sh'))
  }
  if (mode === 'exported-printf') {
    fs.mkdirSync(path.join(directory, 'bin'))
    fs.symlinkSync(bash!, path.join(directory, 'bin', 'sh'))
  }
  const runner = spawn(process.execPath, [startupScript, directory, String(count), mode], {
    detached: true,
    stdio: ['ignore', 'pipe', 'inherit'],
    ...(mode === 'relative-path' ? {
      cwd: path.join(directory, 'runner'),
      env: { ...process.env, PATH: `bin${path.delimiter}${process.env.PATH ?? '/usr/bin:/bin'}` },
    } : {}),
    ...(mode === 'exported-printf' ? {
      env: {
        ...process.env,
        PATH: `${path.join(directory, 'bin')}${path.delimiter}${process.env.PATH ?? '/usr/bin:/bin'}`,
        'BASH_FUNC_printf%%': '() { :; }',
      },
    } : {}),
  })
  let leaders: number[] = []
  try {
    const started = await withDeadline(new Promise<number[]>((resolve) => {
      runner.stdout.once('data', (data: Buffer) => resolve(JSON.parse(data.toString())))
    }), 5_000)
    if (started === 'timed out') throw new Error('The lifecycle spawn did not report its process groups')
    leaders = started
    const markers = Array.from({ length: count }, (_, index) => path.join(directory, `started-${index}.json`))
    const deadline = Date.now() + 5_000
    while (!markers.every((marker) => fs.existsSync(marker)) && Date.now() < deadline) {
      await new Promise<void>((resolve) => setTimeout(resolve, 20)) // eslint-disable-line no-await-in-loop -- wait for the spawned script's marker
    }
    const descendants: number[] = markers.flatMap((marker) => JSON.parse(fs.readFileSync(marker, 'utf8')))
    killProcessGroup(runner.pid!)
    expect(await Promise.all([...leaders, ...descendants].map(async (pid) => endsWithin(pid, 5_000)))).toStrictEqual(Array(count * 3).fill(true))
  } finally {
    killProcessGroup(runner.pid!)
    for (const leader of leaders) killProcessGroup(leader)
    fs.rmSync(directory, { recursive: true, force: true })
  }
}, 15_000)

test('a signal is raised once after concurrent relays settle', async () => {
  const child = { kill: () => true }
  const first = relaySignals(child, { ownProcessGroup: false, terminateOnExit: false })
  const second = relaySignals(child, { ownProcessGroup: false, terminateOnExit: false })
  const originalKill = process.kill
  const raised: Array<[number, string | number | undefined]> = []
  process.kill = ((pid, signal) => {
    raised.push([pid, signal])
    return true
  }) as typeof process.kill
  try {
    await first.settle()
    const firstRaise = first.raise('SIGINT')
    await Promise.resolve()
    expect(raised).toStrictEqual([])

    await second.settle()
    await Promise.all([firstRaise, second.raise('SIGINT')])
    expect(raised).toStrictEqual([[process.pid, 'SIGINT']])
  } finally {
    await Promise.all([first.settle(), second.settle()])
    process.kill = originalKill
  }
})

test('a relay installed while an interrupt is pending is terminated without handling the signal', async () => {
  const child = { kill: () => true }
  const reservation = reserveSignalRelay()
  const first = relaySignals(child, { ownProcessGroup: false, terminateOnExit: false })
  const originalKill = process.kill
  const raised: Array<[number, string | number | undefined]> = []
  process.kill = ((pid, signal) => {
    raised.push([pid, signal])
    return true
  }) as typeof process.kill
  let late: ReturnType<typeof relaySignals> | undefined
  try {
    const firstRaise = first.raise('SIGINT')
    const firstSettle = first.settle()
    const sigintListeners = process.listenerCount('SIGINT')
    const relayed: Array<NodeJS.Signals | number | undefined> = []
    late = relaySignals({ kill: (signal) => {
      relayed.push(signal)
      return true
    } }, { ownProcessGroup: false, terminateOnExit: false })

    expect(relayed).toStrictEqual(['SIGTERM'])
    expect(process.listenerCount('SIGINT')).toBe(sigintListeners)
    const reservationSettle = reservation.settle()
    await Promise.resolve()
    expect(raised).toStrictEqual([])
    await late.settle()
    await Promise.all([firstSettle, firstRaise, reservationSettle])
    expect(raised).toStrictEqual([[process.pid, 'SIGINT']])
  } finally {
    reservation.release()
    await Promise.all([first.settle(), late?.settle()])
    process.kill = originalKill
  }
})

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

testOnLinux('the wait goes on while a zombie of the group lists other threads', async () => {
  const child = spawn('sleep', ['30'], { detached: true, stdio: 'ignore' })
  const group = child.pid!
  try {
    const table = writeProcessTable([{ pid: group, state: 'Z', group, tasks: [group, group + 1] }])
    expect(await withDeadline(waitForProcessGroup(group, { processTable: table }), 500)).toBe('timed out')
  } finally {
    killProcessGroup(group)
  }
})

testOnLinux('the wait ends when a zombie of the group lists only its own thread', async () => {
  const child = spawn('sleep', ['30'], { detached: true, stdio: 'ignore' })
  const group = child.pid!
  try {
    const table = writeProcessTable([{ pid: group, state: 'Z', group, tasks: [group] }])
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

// A released watchdog ends without touching the group, so a process the
// script left behind in it survives pnpm's own exit.
testOnPosix('a released watchdog leaves the group alone', async () => {
  const child = spawn('sleep', ['30'], { detached: true, stdio: 'ignore' })
  const exited = new Promise<string>((resolve) => {
    child.on('exit', () => {
      resolve('exited')
    })
  })
  try {
    watchProcessGroup(child.pid!).release()
    expect(await withDeadline(exited, 500)).toBe('timed out')
  } finally {
    killProcessGroup(child.pid!)
  }
})

testOnPosix('one watchdog watches every process group', async () => {
  const leaders = [spawnGroupLeader(), spawnGroupLeader(), spawnGroupLeader()]
  const runner = spawn(process.execPath, [watchScript, ...leaders.map(({ pid }) => String(pid))], { stdio: ['ignore', 'pipe', 'inherit'] })
  try {
    await once(runner.stdout, 'data')
    const watchdogs = listWatchdogs(runner.pid!)
    expect(watchdogs).toHaveLength(1)
  } finally {
    for (const leader of leaders) killProcessGroup(leader.pid!)
    runner.kill('SIGKILL')
  }
})

// The watchdog outlives the runner that started it. Once the runner dies, it
// kills every group still watched, past one that has exited already, and
// leaves a released group alone.
testOnPosix('the watchdog kills every group still watched when the runner dies', async () => {
  const exited = spawnGroupLeader()
  await new Promise((resolve) => exited.on('exit', resolve).kill('SIGKILL'))
  const released = spawnGroupLeader()
  const leaders = [spawnGroupLeader(), spawnGroupLeader()]
  const runner = spawn(process.execPath, [watchScript, ...[released, exited, ...leaders].map(({ pid }) => String(pid))], { stdio: ['ignore', 'pipe', 'inherit'] })
  try {
    await new Promise((resolve) => runner.stdout.once('data', resolve))
    runner.kill('SIGKILL')
    expect(await Promise.all(leaders.map(async (leader) => endsWithin(leader.pid!, 10_000)))).toStrictEqual([true, true])
    expect(await endsWithin(released.pid!, 500)).toBe(false)
  } finally {
    for (const leader of [released, ...leaders]) killProcessGroup(leader.pid!)
  }
})

// A watch written to the pipe of a watchdog that was just killed is lost, so
// the runner replaces it without waiting for the next watch.
testOnPosix('a killed watchdog is replaced and takes over every group still watched', async () => {
  const released = spawnGroupLeader()
  const leaders = [spawnGroupLeader(), spawnGroupLeader()]
  const runner = spawn(process.execPath, [watchScript, ...[released, ...leaders].map(({ pid }) => String(pid))], { stdio: ['ignore', 'pipe', 'inherit'] })
  try {
    await new Promise((resolve) => runner.stdout.once('data', resolve))
    const [killed] = listWatchdogs(runner.pid!)
    process.kill(killed, 'SIGKILL')
    expect(await waitForReplacement(runner.pid!, killed, 10_000)).toBe(true)
    runner.kill('SIGKILL')
    expect(await Promise.all(leaders.map(async (leader) => endsWithin(leader.pid!, 10_000)))).toStrictEqual([true, true])
    await Promise.all(leaders.map(async (leader) => {
      if (leader.exitCode == null && leader.signalCode == null) await once(leader, 'exit')
    }))
  } finally {
    runner.kill('SIGKILL')
    killProcessGroup(released.pid!)
    for (const leader of leaders) {
      if (leader.exitCode == null && leader.signalCode == null) killProcessGroup(leader.pid!)
    }
  }
})

// The watchdog and its pipe do not keep the runner alive.
testOnPosix('a runner with a watchdog exits on its own', async () => {
  const leader = spawnGroupLeader()
  try {
    const runner = spawn(process.execPath, [watchScript, '--exit', String(leader.pid)], { stdio: 'ignore' })
    const exited = new Promise((resolve) => runner.on('exit', resolve))
    expect(await withDeadline(exited, 5_000)).not.toBe('timed out')
  } finally {
    killProcessGroup(leader.pid!)
  }
})

/** A `sleep` leading a process group of its own, as a script pnpm runs without a terminal does. */
function spawnGroupLeader (): ChildProcess {
  return spawn('sleep', ['30'], { detached: true, stdio: 'ignore' })
}

/** Resolves to whether a watchdog other than `killed` runs under `runner` within `timeout` ms. */
async function waitForReplacement (runner: number, killed: number, timeout: number): Promise<boolean> {
  const deadline = Date.now() + timeout
  while (Date.now() < deadline) {
    if (listWatchdogs(runner).some((watchdog) => watchdog !== killed)) return true
    await new Promise<void>((resolve) => setTimeout(resolve, 50)) // eslint-disable-line no-await-in-loop -- polling: each check must wait for the previous delay
  }
  return false
}

/** The pids of the running watchdogs `parent` started. */
function listWatchdogs (parent: number): number[] {
  const { stdout } = spawnSync('ps', ['-A', '-o', 'pid=', '-o', 'ppid=', '-o', 'args='], { encoding: 'utf8' })
  return stdout.split('\n')
    .map((line) => line.trim().split(/\s+/))
    .filter(([, ppid, ...command]) => Number(ppid) === parent && command.join(' ').includes("trap '' INT TERM HUP"))
    .map(([pid]) => Number(pid))
}

/** A `stat` line is the kernel's: pid, command in parentheses, state, parent, process group. `tasks` are thread ids under `task`. */
function writeProcessTable (entries: Array<{ pid: number, state: string, group: number, readable?: boolean, tasks?: number[] }>): string {
  const table = temporaryDirectory()
  for (const { pid, state, group, readable = true, tasks } of entries) {
    const procDir = path.join(table, String(pid))
    fs.mkdirSync(procDir)
    fs.writeFileSync(path.join(procDir, 'stat'), `${pid} (node) ${state} 1 ${group} ${group}\n`, { mode: readable ? 0o644 : 0o000 })
    for (const tid of tasks ?? []) {
      fs.mkdirSync(path.join(procDir, 'task', String(tid)), { recursive: true })
    }
  }
  return table
}

async function withDeadline<Value> (promise: Promise<Value>, timeout: number): Promise<Value | 'timed out'> {
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
