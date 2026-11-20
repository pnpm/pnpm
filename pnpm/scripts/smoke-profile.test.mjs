import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

// Areas whose breakage a smoke run has to catch. Entries may be re-pointed at
// a different test, but an area must not drop out of the profile.
const REQUIRED_AREAS = ['install', 'add', 'remove', 'update', 'run', 'exec', 'dlx', 'publish']

const repo = path.resolve(fileURLToPath(new URL('../..', import.meta.url)))
const suite = path.join(repo, 'pnpm/crates/cli/tests/suite')

function smokeEntries () {
  const config = fs.readFileSync(path.join(repo, '.config/nextest.toml'), 'utf8')
  const profile = config.slice(config.indexOf('[profile.smoke]'))
  const filter = profile.match(/default-filter = """([\s\S]*?)"""/)
  assert.ok(filter != null, 'the smoke profile must declare a default-filter')
  return [...filter[1].matchAll(/test\(=([\w]+)::([\w]+)\)/g)].map(([, module, name]) => ({ module, name }))
}

function declaresTest (source, name) {
  return new RegExp(`#\\[(?:tokio::)?test[^\\]]*\\]\\s*(?:async\\s+)?fn\\s+${name}\\s*\\(`, 's').test(source)
}

test('every smoke entry names a test that exists', () => {
  for (const { module, name } of smokeEntries()) {
    const file = path.join(suite, `${module}.rs`)
    assert.ok(fs.existsSync(file), `${module}.rs is missing, so test(=${module}::${name}) matches nothing`)
    assert.ok(declaresTest(fs.readFileSync(file, 'utf8'), name),
      `${module}.rs no longer declares ${name}, so test(=${module}::${name}) matches nothing`)
  }
})

test('the smoke profile covers one test per area', () => {
  const modules = smokeEntries().map(entry => entry.module)
  assert.deepEqual(modules, [...new Set(modules)], 'an area should contribute a single smoke test')
})

test('the smoke profile keeps the core areas', () => {
  const modules = new Set(smokeEntries().map(entry => entry.module))
  for (const area of REQUIRED_AREAS) {
    assert.ok(modules.has(area), `the smoke profile no longer covers ${area}`)
  }
})
