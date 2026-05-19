import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { streamParser } from '@pnpm/logger'

import { makeRunPacquet } from '../src/runPacquet.js'

// Skipped on Windows: the test's fake pacquet binary is a JS file with a
// `#!/usr/bin/env node` shebang. Linux and macOS spawn it via the
// shebang; Windows doesn't honor shebangs, so direct `spawn(file)`
// fails with `spawn UNKNOWN`. The production code is unaffected — on
// Windows the real pacquet ships as a native `.exe`.
const testOrSkipOnWindows = process.platform === 'win32' ? test.skip : test

interface FakePacquetOpts {
  ndjsonLines: string[]
  exitCode: number
  stderrTail?: string
}

async function setupFakePacquet (tmpDir: string, opts: FakePacquetOpts): Promise<{ argsPath: string }> {
  const ext = process.platform === 'win32' ? '.exe' : ''
  const binDir = path.join(
    tmpDir,
    `node_modules/.pnpm-config/pacquet/node_modules/@pacquet/${process.platform}-${process.arch}`
  )
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
  const binPath = path.join(binDir, `pacquet${ext}`)
  await fs.promises.writeFile(binPath, script, { mode: 0o755 })
  return { argsPath }
}

testOrSkipOnWindows('makeRunPacquet forwards pacquet NDJSON events through the global streamParser', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-run-pacquet-'))
  try {
    await setupFakePacquet(tmpDir, {
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
      await makeRunPacquet({ lockfileDir: tmpDir, argv: ['install', '--frozen-lockfile'] })()
    } finally {
      streamParser.removeListener('data', reporter)
    }
    const stages = received
      .filter((d): d is { name: string, stage: string } =>
        typeof d === 'object' && d !== null && (d as { name?: string }).name === 'pnpm:stage')
      .map((d) => d.stage)
    expect(stages).toEqual(['importing_started', 'importing_done'])
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

testOrSkipOnWindows('makeRunPacquet forwards the user pnpm argv to pacquet, replacing argv[0] with `install` and ensuring --frozen-lockfile', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-run-pacquet-'))
  try {
    const { argsPath } = await setupFakePacquet(tmpDir, { ndjsonLines: [], exitCode: 0 })
    // `pnpm i --prod` — short alias, no explicit frozen flag. Pacquet
    // doesn't have an `i` alias, and we always need to run it under
    // frozen-lockfile, so both adjustments must apply.
    await makeRunPacquet({ lockfileDir: tmpDir, argv: ['i', '--prod'] })()
    const passedArgs = JSON.parse(await fs.promises.readFile(argsPath, 'utf8'))
    expect(passedArgs).toEqual(['--reporter=ndjson', 'install', '--frozen-lockfile', '--prod'])
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

testOrSkipOnWindows('makeRunPacquet does not duplicate --frozen-lockfile when the user already passed it', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-run-pacquet-'))
  try {
    const { argsPath } = await setupFakePacquet(tmpDir, { ndjsonLines: [], exitCode: 0 })
    await makeRunPacquet({ lockfileDir: tmpDir, argv: ['install', '--frozen-lockfile', '--prod'] })()
    const passedArgs = JSON.parse(await fs.promises.readFile(argsPath, 'utf8'))
    expect(passedArgs).toEqual(['--reporter=ndjson', 'install', '--frozen-lockfile', '--prod'])
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

testOrSkipOnWindows('makeRunPacquet throws PACQUET_INSTALL_FAILED when the binary exits non-zero', async () => {
  const tmpDir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pnpm-run-pacquet-'))
  try {
    await setupFakePacquet(tmpDir, { ndjsonLines: [], exitCode: 1 })
    await expect(makeRunPacquet({ lockfileDir: tmpDir, argv: ['install'] })())
      .rejects.toMatchObject({ code: 'ERR_PNPM_PACQUET_INSTALL_FAILED' })
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})

testOrSkipOnWindows('makeRunPacquet forwards non-JSON stderr lines to the real stderr without breaking parsing', async () => {
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
      await makeRunPacquet({ lockfileDir: tmpDir, argv: ['install'] })()
    } finally {
      streamParser.removeListener('data', reporter)
    }
    const stages = received.filter((d) => d.name === 'pnpm:stage')
    expect(stages).toHaveLength(1)
  } finally {
    await fs.promises.rm(tmpDir, { recursive: true, force: true })
  }
})
