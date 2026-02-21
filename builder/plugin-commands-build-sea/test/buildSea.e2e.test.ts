/**
 * End-to-end test: build a real SEA executable and verify it runs correctly.
 *
 * This test downloads a Node.js binary (or reuses a cached one), builds a
 * Single Executable Application using `node --build-sea`, and then executes
 * the resulting binary to confirm it works.
 *
 * The builder will auto-download Node.js v25 if the current Node.js is older
 * than v25.5 (required for --build-sea support).
 */
import fs from 'fs'
import path from 'path'
import { sync as execaSync } from 'execa'
import { expect, jest, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'
import { handler, type BuildSeaOptions } from '../lib/buildSea.js'

// Map Node.js platform names to pnpm build-sea target OS names
const TARGET_OS: Record<string, string> = {
  linux: 'linux',
  darwin: 'macos',
  win32: 'win',
}

const hostOS = TARGET_OS[process.platform]
const hostArch = process.arch as 'x64' | 'arm64'
const hostTarget = `${hostOS}-${hostArch}`

// Building involves downloading Node.js binaries, which can take some time.
jest.setTimeout(5 * 60 * 1000)

test(`builds and runs a working SEA executable for the host (${hostTarget})`, async () => {
  const tmpDir = tempDir()

  // A minimal CJS entry: uses the node:sea API to verify it's running as a SEA.
  const entryContent = `
const { isSea } = require('node:sea')
process.stdout.write(isSea() ? 'sea:yes' : 'sea:no')
process.exit(0)
`.trimStart()
  fs.writeFileSync('entry.cjs', entryContent)

  const pnpmHomeDir = path.join(tmpDir, 'pnpm-home')

  await handler({
    dir: tmpDir,
    pnpmHomeDir,
    rawConfig: {},
    entry: 'entry.cjs',
    target: hostTarget,
    outputDir: 'out',
    outputName: 'test-sea',
  } as unknown as BuildSeaOptions, [])

  const ext = process.platform === 'win32' ? '.exe' : ''
  const binaryPath = path.join(tmpDir, 'out', hostTarget, `test-sea${ext}`)

  expect(fs.existsSync(binaryPath)).toBe(true)

  // Ensure the binary is executable on Unix
  if (process.platform !== 'win32') {
    fs.chmodSync(binaryPath, 0o755)
  }

  const result = execaSync(binaryPath, [], { stdio: 'pipe' })
  expect(result.stdout.toString()).toBe('sea:yes')
})
