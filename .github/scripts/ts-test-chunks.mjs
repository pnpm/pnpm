import { readFileSync } from 'node:fs'
import path from 'node:path'

// Measured TS test durations for this platform, refreshed with
// update-ts-test-durations.mjs, or undefined when none were recorded.
export function readTestDurations (platform = process.platform) {
  const durations = JSON.parse(readFileSync(path.join(import.meta.dirname, 'ts-test-durations.json'), 'utf8'))
  return durations[platform === 'win32' ? 'windows' : 'linux']
}

// Expected seconds for one task. Without measurements every task falls back
// to its file size, so the weights stay comparable within one run.
export function taskWeight ({ id, size }, durations) {
  if (durations == null) return Math.max(1, size ?? 1)
  return durations.files[id] ?? durations.fallbackSeconds
}

// Greedy longest-first packing: each task goes to the lightest chunk so far.
export function selectChunk (tasks, { chunk, chunks }) {
  const groups = Array.from({ length: chunks }, () => ({ tasks: [], weight: 0 }))
  for (const task of [...tasks].sort(compareTasksByWeight)) {
    const group = groups.reduce((best, candidate) => (
      candidate.weight < best.weight ? candidate : best
    ))
    group.tasks.push(task)
    group.weight += task.weight
  }
  return groups[chunk - 1].tasks.sort((a, b) => a.id.localeCompare(b.id))
}

function compareTasksByWeight (a, b) {
  return b.weight - a.weight || a.id.localeCompare(b.id)
}
