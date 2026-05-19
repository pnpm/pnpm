import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { streamParser } from '@pnpm/logger'

import { runPacquet } from '../../src/install/runPacquet.js'

interface FakePacquetOpts {
  ndjsonLines: string[]
  exitCode: number
  stderrTail?: string
}

async function setupFakePacquet (tmpDir: string, opts: FakePacquetOpts): Promise<{ argsPath: string }> {
  const binDir = path.join(tmpDir, 'node_modules/.pnpm-config/pacquet/bin')
  await fs.promises.mkdir(binDir, { recursive: true })
  const argsPath = path.join(tmpDir, 'fake-pacquet-args.json')
  const lines = opts.ndjsonLines.map((line) => JSON.stringify(line))
  const tail = opts.stderrTail ? JSON.stringify(opts.stderrTail) : null
  const script = [
    '#!/usr/bin/env node',
    `require("node:fs").writeFileSync(${JSON.stringify(argsPath)}, JSON.stringify(process.argv.slice(2)))`,
    `const lines = ${JSON.stringify(lines)}`,
    'for (const line of lines) { process.stderr.write(JSON.parse(line) + "\\n") }',
    tail ? `process.stderr.write(${tail} + "\\n")` : '',
    `process.exit(${opts.exitCode})`,
  ].join('\n')
  const binPath = path.join(binDir, 'pacquet')
  await fs.promises.writeFile(binPath, script, { mode: 0o755 })
  return { argsPath }
}

test('runPacquet forwards pacquet NDJSON events through the global streamParser', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-run-pacquet-'))
  try {
    const { argsPath } = await setupFakePacquet(tmpDir, {
      ndjsonLines: [
        JSON.stringify({ name: 'pnpm:stage', level: 'debug', stage: 'importing_started' }),
        JSON.stringify({ name: 'pnpm:stage', level: 'debug', stage: 'importing_done' }),
      ],
      exitCode: 0,
    })
    const received: unknown[] = []
    const reporter = (data: unknown): void => {
      received.push(data)
    }
    streamParser.on('data', reporter)
    try {
      await runPacquet({ lockfileDir: tmpDir, frozenLockfile: true })
    } finally {
      streamParser.removeListener('data', reporter)
    }
    const stages = received
      .filter((d): d is { name: string, stage: string } =>
        typeof d === 'object' && d !== null && (d as { name?: string }).name === 'pnpm:stage')
      .map((d) => d.stage)
    expect(stages).toEqual(['importing_started', 'importing_done'])
    // Pacquet defaults to silent reporter; the install would emit nothing
    // without the explicit --reporter=ndjson, breaking the reporter UX.
    const passedArgs = JSON.parse(await fs.promises.readFile(argsPath, 'utf8'))
    expect(passedArgs).toEqual(['--reporter=ndjson', 'install', '--frozen-lockfile'])
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

test('runPacquet throws PACQUET_INSTALL_FAILED when the binary exits non-zero', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-run-pacquet-'))
  try {
    await setupFakePacquet(tmpDir, { ndjsonLines: [], exitCode: 1 })
    await expect(runPacquet({ lockfileDir: tmpDir, frozenLockfile: true }))
      .rejects.toMatchObject({ code: 'ERR_PNPM_PACQUET_INSTALL_FAILED' })
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

test('runPacquet forwards non-JSON stderr lines to the real stderr without breaking parsing', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-run-pacquet-'))
  try {
    await setupFakePacquet(tmpDir, {
      ndjsonLines: [
        JSON.stringify({ name: 'pnpm:stage', level: 'debug', stage: 'importing_done' }),
      ],
      stderrTail: 'thread \'main\' panicked at fake panic',
      exitCode: 0,
    })
    const received: Array<{ name?: string }> = []
    const reporter = (data: unknown): void => {
      if (typeof data === 'object' && data !== null) {
        received.push(data as { name?: string })
      }
    }
    streamParser.on('data', reporter)
    try {
      await runPacquet({ lockfileDir: tmpDir, frozenLockfile: true })
    } finally {
      streamParser.removeListener('data', reporter)
    }
    const stages = received.filter((d) => d.name === 'pnpm:stage')
    expect(stages).toHaveLength(1)
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})
