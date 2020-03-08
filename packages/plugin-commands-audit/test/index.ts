import { audit } from '@pnpm/plugin-commands-audit'
import path = require('path')
import stripAnsi = require('strip-ansi')
import test = require('tape')

test('audit', async (t) => {
  const output = await audit.handler({
    dir: path.join(__dirname, 'packages/has-vulnerabilities'),
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })
  t.equal(
    stripAnsi(output),
    `┌─────────────────────┬───────────────────────────────────┐
│ high                │ Insufficient Entropy              │
├─────────────────────┼───────────────────────────────────┤
│ Package             │ cryptiles                         │
├─────────────────────┼───────────────────────────────────┤
│ Vulnerable versions │ <4.1.2                            │
├─────────────────────┼───────────────────────────────────┤
│ Patched versions    │ >=4.1.2                           │
├─────────────────────┼───────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/1464 │
└─────────────────────┴───────────────────────────────────┘
┌─────────────────────┬──────────────────────────────────┐
│ moderate            │ Prototype Pollution              │
├─────────────────────┼──────────────────────────────────┤
│ Package             │ hoek                             │
├─────────────────────┼──────────────────────────────────┤
│ Vulnerable versions │ <= 4.2.0 || >= 5.0.0 < 5.0.3     │
├─────────────────────┼──────────────────────────────────┤
│ Patched versions    │ > 4.2.0 < 5.0.0 || >= 5.0.3      │
├─────────────────────┼──────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/566 │
└─────────────────────┴──────────────────────────────────┘
┌─────────────────────┬──────────────────────────────────┐
│ moderate            │ Memory Exposure                  │
├─────────────────────┼──────────────────────────────────┤
│ Package             │ tunnel-agent                     │
├─────────────────────┼──────────────────────────────────┤
│ Vulnerable versions │ <0.6.0                           │
├─────────────────────┼──────────────────────────────────┤
│ Patched versions    │ >=0.6.0                          │
├─────────────────────┼──────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/598 │
└─────────────────────┴──────────────────────────────────┘
┌─────────────────────┬──────────────────────────────────┐
│ moderate            │ Denial of Service                │
├─────────────────────┼──────────────────────────────────┤
│ Package             │ axios                            │
├─────────────────────┼──────────────────────────────────┤
│ Vulnerable versions │ <0.18.1                          │
├─────────────────────┼──────────────────────────────────┤
│ Patched versions    │ >=0.18.1                         │
├─────────────────────┼──────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/880 │
└─────────────────────┴──────────────────────────────────┘
┌─────────────────────┬──────────────────────────────────────┐
│ low                 │ Regular Expression Denial of Service │
├─────────────────────┼──────────────────────────────────────┤
│ Package             │ timespan                             │
├─────────────────────┼──────────────────────────────────────┤
│ Vulnerable versions │ >=0.0.0                              │
├─────────────────────┼──────────────────────────────────────┤
│ Patched versions    │ <0.0.0                               │
├─────────────────────┼──────────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/533     │
└─────────────────────┴──────────────────────────────────────┘
┌─────────────────────┬──────────────────────────────────────┐
│ low                 │ Regular Expression Denial of Service │
├─────────────────────┼──────────────────────────────────────┤
│ Package             │ braces                               │
├─────────────────────┼──────────────────────────────────────┤
│ Vulnerable versions │ <2.3.1                               │
├─────────────────────┼──────────────────────────────────────┤
│ Patched versions    │ >=2.3.1                              │
├─────────────────────┼──────────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/786     │
└─────────────────────┴──────────────────────────────────────┘
6 vulnerabilities found
Severity: 2 low | 3 moderate | 1 high`)
  t.end()
})

test('audit --dev', async (t) => {
  const output = await audit.handler({
    dir: path.join(__dirname, 'packages/has-vulnerabilities'),
    include: {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    },
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  t.equal(
    stripAnsi(output),
    `┌─────────────────────┬──────────────────────────────────┐
│ moderate            │ Denial of Service                │
├─────────────────────┼──────────────────────────────────┤
│ Package             │ axios                            │
├─────────────────────┼──────────────────────────────────┤
│ Vulnerable versions │ <0.18.1                          │
├─────────────────────┼──────────────────────────────────┤
│ Patched versions    │ >=0.18.1                         │
├─────────────────────┼──────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/880 │
└─────────────────────┴──────────────────────────────────┘
1 vulnerabilities found
Severity: 1 moderate`)
  t.end()
})

test('audit --audit-level', async (t) => {
  const output = await audit.handler({
    auditLevel: 'moderate',
    dir: path.join(__dirname, 'packages/has-vulnerabilities'),
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  t.equal(
    stripAnsi(output),
      `┌─────────────────────┬───────────────────────────────────┐
│ high                │ Insufficient Entropy              │
├─────────────────────┼───────────────────────────────────┤
│ Package             │ cryptiles                         │
├─────────────────────┼───────────────────────────────────┤
│ Vulnerable versions │ <4.1.2                            │
├─────────────────────┼───────────────────────────────────┤
│ Patched versions    │ >=4.1.2                           │
├─────────────────────┼───────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/1464 │
└─────────────────────┴───────────────────────────────────┘
┌─────────────────────┬──────────────────────────────────┐
│ moderate            │ Prototype Pollution              │
├─────────────────────┼──────────────────────────────────┤
│ Package             │ hoek                             │
├─────────────────────┼──────────────────────────────────┤
│ Vulnerable versions │ <= 4.2.0 || >= 5.0.0 < 5.0.3     │
├─────────────────────┼──────────────────────────────────┤
│ Patched versions    │ > 4.2.0 < 5.0.0 || >= 5.0.3      │
├─────────────────────┼──────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/566 │
└─────────────────────┴──────────────────────────────────┘
┌─────────────────────┬──────────────────────────────────┐
│ moderate            │ Memory Exposure                  │
├─────────────────────┼──────────────────────────────────┤
│ Package             │ tunnel-agent                     │
├─────────────────────┼──────────────────────────────────┤
│ Vulnerable versions │ <0.6.0                           │
├─────────────────────┼──────────────────────────────────┤
│ Patched versions    │ >=0.6.0                          │
├─────────────────────┼──────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/598 │
└─────────────────────┴──────────────────────────────────┘
┌─────────────────────┬──────────────────────────────────┐
│ moderate            │ Denial of Service                │
├─────────────────────┼──────────────────────────────────┤
│ Package             │ axios                            │
├─────────────────────┼──────────────────────────────────┤
│ Vulnerable versions │ <0.18.1                          │
├─────────────────────┼──────────────────────────────────┤
│ Patched versions    │ >=0.18.1                         │
├─────────────────────┼──────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/880 │
└─────────────────────┴──────────────────────────────────┘
6 vulnerabilities found
Severity: 2 low | 3 moderate | 1 high`)
  t.end()
})

test('audit: no vulnerabilities', async (t) => {
  const output = await audit.handler({
    dir: path.join(__dirname, '../../../fixtures/has-outdated-deps'),
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  t.equal(stripAnsi(output), 'No known vulnerabilities found')
  t.end()
})

test('audit --json', async (t) => {
  const output = await audit.handler({
    dir: path.join(__dirname, 'packages/has-vulnerabilities'),
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    json: true,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  })

  const json = JSON.parse(output)
  t.ok(json.metadata)
  t.end()
})
