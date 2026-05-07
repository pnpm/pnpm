import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@jest/globals'

const BIN = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'bin', 'pnpm-render.mjs')

test('pnpm-render bin renders ndjson piped from stdin', async () => {
  const lines = [
    { name: 'pnpm:stage', level: 'debug', prefix: '/tmp/proj', stage: 'resolution_started' },
    { name: 'pnpm:progress', level: 'debug', packageId: 'foo@1.0.0', requester: '/tmp/proj', status: 'resolved' },
    { name: 'pnpm:progress', level: 'debug', packageId: 'bar@2.0.0', requester: '/tmp/proj', status: 'resolved' },
    { name: 'pnpm:summary', level: 'debug', prefix: '/tmp/proj' },
  ]

  const stdout = await runBin(['install'], lines.map((line) => JSON.stringify(line)).join('\n') + '\n')

  // ansi-diff intersperses cursor-movement escapes when the rendered string
  // changes (e.g. "1" → "2"), so we can't substring-match the final value
  // without terminal emulation. Verifying that any progress line rendered is
  // enough to prove the bin wired stdin → reporter correctly.
  expect(stdout).toContain('Progress: resolved')
})

test('pnpm-render bin ignores malformed and non-object stdin lines', async () => {
  const stdout = await runBin(['install'], [
    'not-json',
    'null',
    '42',
    '"a string"',
    JSON.stringify({ name: 'pnpm:stage', level: 'debug', prefix: '/tmp/proj', stage: 'resolution_started' }),
    JSON.stringify({ name: 'pnpm:progress', level: 'debug', packageId: 'foo@1.0.0', requester: '/tmp/proj', status: 'resolved' }),
    '',
    JSON.stringify({ name: 'pnpm:summary', level: 'debug', prefix: '/tmp/proj' }),
  ].join('\n') + '\n')

  expect(stdout).toContain('Progress: resolved 1')
})

async function runBin (args: readonly string[], stdin: string): Promise<string> {
  const child = spawn(process.execPath, [BIN, ...args], { stdio: ['pipe', 'pipe', 'pipe'] })
  let stdout = ''
  let stderr = ''
  child.stdout.on('data', (chunk: Buffer) => {
    stdout += chunk.toString()
  })
  child.stderr.on('data', (chunk: Buffer) => {
    stderr += chunk.toString()
  })
  child.stdin.end(stdin)
  // Wait for 'close' (not 'exit'): 'exit' can fire before stdout/stderr
  // are fully drained, which leads to truncated captures and flaky asserts.
  const exitCode = await new Promise<number | null>((resolve, reject) => {
    child.on('error', reject)
    child.on('close', (code) => {
      resolve(code)
    })
  })
  if (exitCode !== 0) {
    throw new Error(`pnpm-render exited with ${exitCode}\nstdout:\n${stdout}\nstderr:\n${stderr}`)
  }
  return stdout
}
