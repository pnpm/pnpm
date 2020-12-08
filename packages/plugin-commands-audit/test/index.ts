import { audit } from '@pnpm/plugin-commands-audit'
import path = require('path')
import stripAnsi = require('strip-ansi')

test('audit', async () => {
  if (process.version.split('.')[0] === 'v10') {
    // The audits give different results on Node 10, for some reason
    return
  }
  const { output, exitCode } = await audit.handler({
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
  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`┌─────────────────────┬───────────────────────────────────┐
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
┌─────────────────────┬───────────────────────────────────┐
│ high                │ Denial of Service                 │
├─────────────────────┼───────────────────────────────────┤
│ Package             │ http-proxy                        │
├─────────────────────┼───────────────────────────────────┤
│ Vulnerable versions │ <1.18.1                           │
├─────────────────────┼───────────────────────────────────┤
│ Patched versions    │ >=1.18.1                          │
├─────────────────────┼───────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/1486 │
└─────────────────────┴───────────────────────────────────┘
┌─────────────────────┬───────────────────────────────────────────────────────────────┐
│ high                │ Remote Memory Exposure                                        │
├─────────────────────┼───────────────────────────────────────────────────────────────┤
│ Package             │ bl                                                            │
├─────────────────────┼───────────────────────────────────────────────────────────────┤
│ Vulnerable versions │ <1.2.3 || >2.0.0 < 2.2.1 || >=3.0.0 <3.0.1 || >= 4.0.0 <4.0.3 │
├─────────────────────┼───────────────────────────────────────────────────────────────┤
│ Patched versions    │ >=1.2.3 <2.0.0 || >=2.2.1 <3.0.0 || >=3.0.1 <4.0.0 || >=4.0.3 │
├─────────────────────┼───────────────────────────────────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/1555                             │
└─────────────────────┴───────────────────────────────────────────────────────────────┘
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
┌─────────────────────┬───────────────────────────────────┐
│ low                 │ Prototype Pollution               │
├─────────────────────┼───────────────────────────────────┤
│ Package             │ minimist                          │
├─────────────────────┼───────────────────────────────────┤
│ Vulnerable versions │ <0.2.1 || >=1.0.0 <1.2.3          │
├─────────────────────┼───────────────────────────────────┤
│ Patched versions    │ >=0.2.1 <1.0.0 || >=1.2.3         │
├─────────────────────┼───────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/1179 │
└─────────────────────┴───────────────────────────────────┘
┌─────────────────────┬───────────────────────────────────┐
│ low                 │ Validation Bypass                 │
├─────────────────────┼───────────────────────────────────┤
│ Package             │ kind-of                           │
├─────────────────────┼───────────────────────────────────┤
│ Vulnerable versions │ >=6.0.0 <6.0.3                    │
├─────────────────────┼───────────────────────────────────┤
│ Patched versions    │ >=6.0.3                           │
├─────────────────────┼───────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/1490 │
└─────────────────────┴───────────────────────────────────┘
┌─────────────────────┬───────────────────────────────────┐
│ low                 │ Prototype Pollution               │
├─────────────────────┼───────────────────────────────────┤
│ Package             │ lodash                            │
├─────────────────────┼───────────────────────────────────┤
│ Vulnerable versions │ <4.17.19                          │
├─────────────────────┼───────────────────────────────────┤
│ Patched versions    │ >=4.17.19                         │
├─────────────────────┼───────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/1523 │
└─────────────────────┴───────────────────────────────────┘
12 vulnerabilities found
Severity: 6 low | 3 moderate | 3 high`)
})

test('audit --dev', async () => {
  const { output, exitCode } = await audit.handler({
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

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`┌─────────────────────┬──────────────────────────────────┐
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
})

test('audit --audit-level', async () => {
  const { output, exitCode } = await audit.handler({
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

  expect(exitCode).toBe(1)
  expect(stripAnsi(output)).toBe(`┌─────────────────────┬───────────────────────────────────┐
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
┌─────────────────────┬───────────────────────────────────┐
│ high                │ Denial of Service                 │
├─────────────────────┼───────────────────────────────────┤
│ Package             │ http-proxy                        │
├─────────────────────┼───────────────────────────────────┤
│ Vulnerable versions │ <1.18.1                           │
├─────────────────────┼───────────────────────────────────┤
│ Patched versions    │ >=1.18.1                          │
├─────────────────────┼───────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/1486 │
└─────────────────────┴───────────────────────────────────┘
┌─────────────────────┬───────────────────────────────────────────────────────────────┐
│ high                │ Remote Memory Exposure                                        │
├─────────────────────┼───────────────────────────────────────────────────────────────┤
│ Package             │ bl                                                            │
├─────────────────────┼───────────────────────────────────────────────────────────────┤
│ Vulnerable versions │ <1.2.3 || >2.0.0 < 2.2.1 || >=3.0.0 <3.0.1 || >= 4.0.0 <4.0.3 │
├─────────────────────┼───────────────────────────────────────────────────────────────┤
│ Patched versions    │ >=1.2.3 <2.0.0 || >=2.2.1 <3.0.0 || >=3.0.1 <4.0.0 || >=4.0.3 │
├─────────────────────┼───────────────────────────────────────────────────────────────┤
│ More info           │ https://npmjs.com/advisories/1555                             │
└─────────────────────┴───────────────────────────────────────────────────────────────┘
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
12 vulnerabilities found
Severity: 6 low | 3 moderate | 3 high`)
})

test('audit: no vulnerabilities', async () => {
  const { output, exitCode } = await audit.handler({
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

  expect(stripAnsi(output)).toBe('No known vulnerabilities found')
  expect(exitCode).toBe(0)
})

test('audit --json', async () => {
  const { output, exitCode } = await audit.handler({
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
  expect(json.metadata).toBeTruthy()
  expect(exitCode).toBe(1)
})

test('audit does not exit with code 1 if the found vulnerabilities are having lower severity then what we asked for', async () => {
  const { output, exitCode } = await audit.handler({
    auditLevel: 'high',
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

  expect(exitCode).toBe(0)
  expect(stripAnsi(output)).toBe(`1 vulnerabilities found
Severity: 1 moderate`)
})
