import assert from 'node:assert/strict'
import fs from 'node:fs'
import { test } from 'node:test'
import { runRustTests } from './run-rust-tests.mjs'

function recordingSpawn (status = 0) {
  const calls = []
  return {
    calls,
    spawn (command, args, options) {
      const { env } = options
      calls.push({
        command,
        args,
        env,
        cacheDirExisted: fs.existsSync(env.XDG_CACHE_HOME),
        configDirExisted: fs.existsSync(env.XDG_CONFIG_HOME),
      })
      return { status }
    },
  }
}

test('runs nextest with a cache and a config directory of its own, removed afterwards', () => {
  const { calls, spawn } = recordingSpawn(3)
  const env = { PATH: '/bin', XDG_CACHE_HOME: '/home/user/.cache', XDG_CONFIG_HOME: '/home/user/.config' }

  assert.equal(runRustTests(['-p', 'pnpm-config'], { spawn, env }), 3)

  assert.equal(calls.length, 1)
  const [call] = calls
  assert.equal(call.command, 'cargo')
  assert.deepEqual(call.args, ['nextest', 'run', '-p', 'pnpm-config'])
  assert.notEqual(call.env.XDG_CACHE_HOME, env.XDG_CACHE_HOME)
  assert.notEqual(call.env.XDG_CONFIG_HOME, env.XDG_CONFIG_HOME)
  assert.ok(call.cacheDirExisted, 'the cache directory exists while the tests run')
  assert.ok(call.configDirExisted, 'the config directory exists while the tests run')
  assert.ok(!fs.existsSync(call.env.XDG_CACHE_HOME), 'the cache directory is removed after the run')
  assert.ok(!fs.existsSync(call.env.XDG_CONFIG_HOME), 'the config directory is removed after the run')
})

test('drops npm and pnpm configuration from the environment', () => {
  const { calls, spawn } = recordingSpawn()
  const env = { PATH: '/bin', npm_config_registry: 'https://example.test', PNPM_CONFIG_STORE_DIR: '/store' }

  runRustTests([], { spawn, env })

  const [{ env: passed }] = calls
  assert.equal(passed.npm_config_registry, undefined)
  assert.equal(passed.PNPM_CONFIG_STORE_DIR, undefined)
  assert.equal(passed.PNPM_CONFIG_CI, 'false')
  assert.equal(passed.PATH, '/bin')
})
