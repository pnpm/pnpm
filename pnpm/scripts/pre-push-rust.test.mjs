import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { temporaryRepo } from './git-fixture.mjs'

const SCRIPT = path.join(path.dirname(fileURLToPath(import.meta.url)), 'pre-push-rust.sh')

// Every tool the script runs is a stub, so the test exercises only the
// script's own logic. The `cargo` stub records the git variables it sees when
// asked to run dylint.
function stubbedCheckout (context) {
  const dir = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-pre-push-')))
  context.after(() => fs.rmSync(dir, { recursive: true, force: true }))
  const bin = path.join(dir, 'bin')
  fs.mkdirSync(bin)
  const record = path.join(dir, 'dylint-env')
  const stub = (name, body) => fs.writeFileSync(path.join(bin, name), `#!/bin/sh\n${body}\nexit 0\n`, { mode: 0o755 })
  stub('cargo', `if [ "$1" = dylint ]; then env | grep '^GIT_' > '${record}'; fi`)
  stub('cargo-dylint', '')
  stub('typos', '')
  stub('taplo', '')
  fs.mkdirSync(path.join(dir, 'pnpm/scripts'), { recursive: true })
  fs.writeFileSync(path.join(dir, 'pnpm/scripts/rustfmt.mjs'), '')
  return { dir, bin, record }
}

test('runs cargo dylint without the git variables a hook inherits', { skip: process.platform === 'win32' }, (context) => {
  const repo = temporaryRepo(context, 'pnpm-pre-push-repo-')
  const { dir, bin, record } = stubbedCheckout(context)

  const result = spawnSync('bash', [SCRIPT], {
    cwd: dir,
    encoding: 'utf8',
    env: {
      PATH: [bin, path.dirname(process.execPath), '/usr/bin', '/bin'].join(path.delimiter),
      GIT_DIR: path.join(repo, '.git'),
      GIT_INDEX_FILE: path.join(repo, '.git/index'),
      GIT_WORK_TREE: repo,
    },
  })

  assert.equal(result.status, 0, result.stderr)
  assert.equal(fs.readFileSync(record, 'utf8'), '')
})
