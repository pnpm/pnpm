import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@jest/globals'

const BIN = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'bin', 'pnpm-render.mjs')

test('pnpm-render bin renders ndjson piped from stdin', async () => {
  const lines = [
    { name: 'pnpm:stage', level: 'debug', prefix: '/tmp/proj', stage: 'resolution_started' },
    { name: 'pnpm:progress', level: 'debug', packageId: 'foo@1.0.0', requester: '/tmp/proj', status: 'resolved' },
    { name: 'pnpm:summary', level: 'debug', prefix: '/tmp/proj' },
  ]

  const stdout = await runBin(['install'], lines.map((line) => JSON.stringify(line)).join('\n') + '\n')

  expect(stdout).toContain('Progress: resolved')
})

test('pnpm-render bin ignores non-JSON lines on stdin', async () => {
  const stdout = await runBin(['install'], [
    'not-json',
    JSON.stringify({ name: 'pnpm:stage', level: 'debug', prefix: '/tmp/proj', stage: 'resolution_started' }),
    JSON.stringify({ name: 'pnpm:progress', level: 'debug', packageId: 'foo@1.0.0', requester: '/tmp/proj', status: 'resolved' }),
    '',
    JSON.stringify({ name: 'pnpm:summary', level: 'debug', prefix: '/tmp/proj' }),
  ].join('\n') + '\n')

  expect(stdout).toContain('Progress: resolved')
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
  const exitCode = await new Promise<number | null>((resolve, reject) => {
    child.on('error', reject)
    child.on('exit', (code) => {
      resolve(code)
    })
  })
  if (exitCode !== 0) {
    throw new Error(`pnpm-render exited with ${exitCode}\nstdout:\n${stdout}\nstderr:\n${stderr}`)
  }
  return stdout
}
