import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'

// A stalled test has to be terminated, not just marked SLOW. Without
// `terminate-after`, nextest waits for it forever and a single hung test
// binary wedges the whole run until the CI job is killed from outside, with
// no output from the offender (pnpm/tasks#32: an 80-minute job).
//
// Profiles other than `default` inherit from it, so a profile that declares
// no `slow-timeout` is covered. One that declares its own value is not: the
// declaration is what a run of that profile uses. A per-test override is the
// same: it supplies the whole `slow-timeout` value, so an override that names
// only a period drops the termination policy for the tests it matches.
const repo = path.resolve(fileURLToPath(new URL('../..', import.meta.url)))

const OVERRIDE_SECTION = /^\[\[profile\.[^\]]+\.overrides\]\]\s*$/gm

function profiles () {
  const config = fs.readFileSync(path.join(repo, '.config/nextest.toml'), 'utf8')
  const sections = [...config.matchAll(/^\[profile\.([^\]]+)\]\s*$/gm)]
  return sections.map((section, index) => ({
    name: section[1],
    body: config.slice(section.index + section[0].length, sections[index + 1]?.index ?? config.length),
  }))
}

function splitOverrides (body) {
  const sections = [...body.matchAll(OVERRIDE_SECTION)]
  if (sections.length === 0) return { base: body, overrides: [] }
  return {
    base: body.slice(0, sections[0].index),
    overrides: sections.map((section, index) => body.slice(
      section.index + section[0].length,
      sections[index + 1]?.index ?? body.length,
    )),
  }
}

function declaredSlowTimeout (body) {
  const line = body.match(/^slow-timeout\s*=\s*(.+)$/m)
  return line == null ? null : line[1].split('#')[0].trim()
}

const REQUIRED = 'declare `terminate-after`, so a stalled test is terminated'

test('the default profile terminates a stalled test', () => {
  const profile = profiles().find(candidate => candidate.name === 'default')
  assert.ok(profile != null, 'the default profile must exist')
  const value = declaredSlowTimeout(splitOverrides(profile.body).base)
  assert.ok(value != null, `the default profile must ${REQUIRED}`)
  assert.match(value, /terminate-after/,
    `the default profile must ${REQUIRED}: a \`slow-timeout\` alone only marks the test SLOW and waits forever`)
})

test('a profile or override that sets its own slow-timeout also terminates a stalled test', () => {
  for (const profile of profiles()) {
    const { base, overrides } = splitOverrides(profile.body)
    const declared = [
      { source: `[profile.${profile.name}]`, value: declaredSlowTimeout(base) },
      ...overrides.map((body, index) => ({
        source: `[profile.${profile.name}.overrides] (${index + 1} of ${overrides.length})`,
        value: declaredSlowTimeout(body),
      })),
    ]
    for (const { source, value } of declared) {
      if (value == null) continue
      assert.match(value, /terminate-after/,
        `${source} must ${REQUIRED}: it declares its own value, so it cannot rely on the one it inherits`)
    }
  }
})
