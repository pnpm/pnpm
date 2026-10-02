import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test } from 'node:test'

const workflow = readFileSync(new URL('../workflows/test.yml', import.meta.url), 'utf8')
const step = workflow.split('    - name: Wait for prebuilt pnpr binaries\n')[1]?.split('    - name: ')[0]
const run = step?.split('      run: |\n')[1]
assert.ok(run, 'The pnpr polling step must exist')
const script = run.split('\n').map(line => line.slice(8)).join('\n')

const stubs = `
gh() {
  echo call >> "$POLL_TEST_DIR/calls"
  if [ "$API_ERROR" = true ] && [ ! -f "$POLL_TEST_DIR/retried" ]; then
    touch "$POLL_TEST_DIR/retried"
    printf '{\n  "message": "Server Error"\n}\n'
    echo 'gh: Server Error (HTTP 502)' >&2
    return 1
  fi
  printf '%s\n' "$BUILD_CONCLUSION"
}
sleep() {
  [ "$1" = 15 ] || return 1
  echo retry >> "$POLL_TEST_DIR/sleeps"
}
`

for (const runnerOs of ['Linux', 'Windows']) {
  for (const apiError of [false, true]) {
    for (const conclusion of ['success', 'failure', 'cancelled', 'timed_out']) {
      test(`${runnerOs}: ${conclusion}, API error: ${apiError}`, () => {
        const dir = mkdtempSync(path.join(tmpdir(), 'pnpr-polling-'))
        try {
          const result = spawnSync('bash', ['--noprofile', '--norc', '-e', '-o', 'pipefail', '-c', stubs + script], {
            encoding: 'utf8',
            timeout: 5000,
            env: {
              ...process.env,
              POLL_TEST_DIR: dir,
              API_ERROR: String(apiError),
              BUILD_CONCLUSION: conclusion,
              RUNNER_OS_NAME: runnerOs,
              GITHUB_REPOSITORY: 'pnpm/pnpm',
              GITHUB_RUN_ID: '123',
            },
          })
          assert.ifError(result.error)
          assert.equal(result.status, conclusion === 'success' ? 0 : 1, result.stdout + result.stderr)
          assert.equal(readFileSync(path.join(dir, 'calls'), 'utf8'), apiError ? 'call\ncall\n' : 'call\n')
          if (apiError) assert.equal(readFileSync(path.join(dir, 'sleeps'), 'utf8'), 'retry\n')
          if (conclusion !== 'success') {
            assert.match(result.stdout, new RegExp(`::error::The ${runnerOs} pnpr build ended with ${conclusion}`))
          }
        } finally {
          rmSync(dir, { recursive: true, force: true })
        }
      })
    }
  }
}
