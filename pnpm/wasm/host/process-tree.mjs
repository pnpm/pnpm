import { execFile } from 'node:child_process'
import { promisify } from 'node:util'

const executeFile = promisify(execFile)

export async function killProcessTree (child, signal) {
  if (!isRunning(child)) return false
  let stdout
  try {
    ({ stdout } = await executeFile('/bin/ps', ['-eo', 'pid,ppid'], {
      timeout: 2000, maxBuffer: 4 * 1024 * 1024, encoding: 'utf8',
    }))
  } catch (error) {
    if (isRunning(child)) child.kill(signal)
    throw error
  }
  if (!isRunning(child)) return false
  const descendants = findDescendants(stdout, child.pid)
  for (const pid of descendants.reverse()) signalProcess(pid, signal)
  return child.kill(signal)
}

export function findDescendants (listing, parentPid) {
  const children = new Map()
  for (const line of listing.trim().split('\n').slice(1)) {
    const [pid, parent] = line.trim().split(/\s+/).map(Number)
    if (!Number.isInteger(pid) || pid < 1 || !Number.isInteger(parent) || parent < 0) continue
    const siblings = children.get(parent) ?? []
    siblings.push(pid)
    children.set(parent, siblings)
  }
  const descendants = []
  const visited = new Set([parentPid])
  const pending = [...(children.get(parentPid) ?? [])]
  for (const pid of pending) {
    if (visited.has(pid)) continue
    visited.add(pid)
    descendants.push(pid)
    pending.push(...(children.get(pid) ?? []))
  }
  return descendants
}

function isRunning (child) {
  return child.pid !== undefined && child.exitCode === null && child.signalCode === null
}

function signalProcess (pid, signal) {
  try {
    process.kill(pid, signal)
  } catch (error) {
    if (error.code !== 'ESRCH') throw error
  }
}
