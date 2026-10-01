import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath, pathToFileURL } from 'node:url'

import { parallelismEnv } from './cargo-jobs.mjs'

/**
 * Run `cargo nextest run` with `args` in an environment that reads no npm or
 * pnpm configuration of the user's and writes nothing to the user's pnpm
 * cache: each run gets its own config and cache directories, removed after
 * it. Returns the exit code.
 */
export function runRustTests (args, { spawn = spawnSync, env: parentEnv = process.env } = {}) {
  const configDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-test-config-'))
  const cacheDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-test-cache-'))
  try {
    const env = Object.fromEntries(Object.entries(parentEnv).filter(([name]) => {
      const lowerName = name.toLowerCase()
      return !lowerName.startsWith('npm_config_') && !lowerName.startsWith('pnpm_config_')
    }))
    const npmrcPath = path.join(configDir, 'npmrc')
    fs.writeFileSync(npmrcPath, '')
    Object.assign(env, parallelismEnv(env), {
      PNPM_CONFIG_CI: 'false',
      PNPM_CONFIG_NPMRC_AUTH_FILE: npmrcPath,
      PNPM_TEST_NPMRC_AUTH_FILE: npmrcPath,
      XDG_CACHE_HOME: cacheDir,
      XDG_CONFIG_HOME: configDir,
    })

    const result = spawn('cargo', ['nextest', 'run', ...args], {
      env,
      stdio: 'inherit',
    })
    if (result.error != null) throw result.error
    return result.status ?? 1
  } finally {
    fs.rmSync(configDir, { recursive: true, force: true })
    fs.rmSync(cacheDir, { recursive: true, force: true })
  }
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    process.exitCode = runRustTests(process.argv.slice(2))
  } catch (error) {
    process.stderr.write(`${path.basename(fileURLToPath(import.meta.url))}: ${error.message}\n`)
    process.exitCode = 1
  }
}
