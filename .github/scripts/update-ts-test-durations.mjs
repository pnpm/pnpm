#!/usr/bin/env node
// Refresh ts-test-durations.json from the chunked TS CI test jobs of one run:
//   node .github/scripts/update-ts-test-durations.mjs <run-id>
// Jest prints a duration only for files slower than its 5 s threshold, so the
// file records those, plus the average cost of every other file.
import { execFileSync } from 'node:child_process'
import { writeFileSync } from 'node:fs'
import path from 'node:path'

const runId = process.argv[2]
if (!/^\d+$/.test(runId ?? '')) {
  console.error('usage: update-ts-test-durations.mjs <run-id>')
  process.exit(1)
}

const repo = process.env.GITHUB_REPOSITORY ?? 'pnpm/pnpm'
const jobs = JSON.parse(execFileSync('gh', [
  'api', '--paginate', '--slurp', `repos/${repo}/actions/runs/${runId}/jobs?per_page=100`,
], { encoding: 'utf8' })).flatMap((page) => page.jobs)

// One Node.js version per platform keeps each file measured once.
const platformOf = (name) => {
  if (/Test \/ windows .*Node 22 \/ chunk/.test(name)) return 'windows'
  if (/Test \/ ubuntu .*Node 22 \/ chunk/.test(name)) return 'linux'
  return undefined
}

const platforms = {}
for (const job of jobs) {
  const platform = platformOf(job.name)
  if (platform == null || job.conclusion !== 'success') continue
  const measured = platforms[platform] ??= { files: {}, stepSeconds: 0, taskCount: 0 }
  const testStep = job.steps.find((step) => step.name.startsWith('Run tests'))
  measured.stepSeconds += (Date.parse(testStep.completed_at) - Date.parse(testStep.started_at)) / 1000
  const log = execFileSync('gh', ['run', 'view', '--repo', repo, '--job', String(job.id), '--log'], {
    encoding: 'utf8',
    maxBuffer: 512 * 1024 * 1024,
  })
  let packageDir
  for (const line of log.split('\n')) {
    const selected = /Selected (\d+) of \d+ test tasks/.exec(line)
    if (selected != null) measured.taskCount += Number(selected[1])
    const started = /Running \d+ Jest file\(s\) in (\S+)/.exec(line)
    if (started != null) packageDir = started[1]
    const passed = /\bPASS (\S+) \((\d+(?:\.\d+)?) s\)/.exec(line)
    if (passed != null && packageDir != null) {
      measured.files[path.posix.join(packageDir, passed[1].replaceAll('\\', '/'))] = Math.round(Number(passed[2]))
    }
  }
}

const durations = {}
for (const [platform, measured] of Object.entries(platforms).sort()) {
  const files = Object.entries(measured.files)
  const slowSeconds = files.reduce((sum, [, seconds]) => sum + seconds, 0)
  durations[platform] = {
    fallbackSeconds: Math.round((measured.stepSeconds - slowSeconds) / (measured.taskCount - files.length) * 100) / 100,
    files: Object.fromEntries(files.sort(([a], [b]) => a.localeCompare(b))),
  }
}
const output = path.join(import.meta.dirname, 'ts-test-durations.json')
writeFileSync(output, `${JSON.stringify(durations, null, 2)}\n`)
for (const [platform, { fallbackSeconds, files }] of Object.entries(durations)) {
  console.log(`${platform}: ${Object.keys(files).length} measured files, ${fallbackSeconds} s for every other file`)
}
