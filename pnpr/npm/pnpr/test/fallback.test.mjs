import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import { getBinCandidates } from '../install.js'

const pnprNpmDir = path.resolve(fileURLToPath(import.meta.url), '../..')
const binPnpr = path.join(pnprNpmDir, 'bin/pnpr')
const binPnprMjs = path.join(pnprNpmDir, 'bin/pnpr.mjs')

test('bin/pnpr and bin/pnpr.mjs fail gracefully when native binary is missing', () => {
  const resultSh = spawnSync('sh', [binPnpr], { encoding: 'utf8' })
  assert.equal(resultSh.status, 1)
  assert.match(resultSh.stderr, /has no native binary installed for this platform/)

  const resultNode = spawnSync(process.execPath, [binPnprMjs], { encoding: 'utf8' })
  assert.equal(resultNode.status, 1)
  assert.match(resultNode.stderr, /has no native binary installed for this platform/)
})

test('bin/pnpr spawns native binary when candidate is found', (t) => {
  const candidates = getBinCandidates()
  if (candidates.length === 0) return

  const candidate = candidates[0]
  const targetDir = path.join(pnprNpmDir, 'node_modules', path.dirname(candidate))
  const targetFile = path.join(pnprNpmDir, 'node_modules', candidate)

  assert.ok(!fs.existsSync(targetDir), `${targetDir} already exists`)
  fs.mkdirSync(targetDir, { recursive: true })
  fs.writeFileSync(path.join(targetDir, 'package.json'), JSON.stringify({ name: path.dirname(candidate) }))
  fs.writeFileSync(targetFile, '#!/bin/sh\necho "mock pnpr running: $*"')
  fs.chmodSync(targetFile, 0o755)

  t.after(() => {
    fs.rmSync(targetDir, { recursive: true, force: true })
  })

  const result = spawnSync('sh', [binPnpr, 'serve', '--port', '1234'], { encoding: 'utf8' })
  assert.equal(result.status, 0)
  assert.match(result.stdout, /mock pnpr running: serve --port 1234/)
})
