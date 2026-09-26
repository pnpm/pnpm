import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpmSync } from '../utils/index.js'

const reporterEnv = { pnpm_config_silent: 'false' }
const failure = '@pnpm.e2e/failing-postinstall'
const success = '@pnpm.e2e/install-script-example'

test.each<[string, string | undefined]>([
  ['warn', undefined], ['error', undefined],
  ['warn', 'default'], ['error', 'default'],
  ['warn', 'append-only'], ['error', 'append-only'],
])('root install failures retain full diagnostics at %s with reporter %s in resolved and frozen installs', (level, reporter) => {
  prepare({
    dependencies: { 'is-positive': '1.0.0' },
    scripts: { postinstall: 'node diagnostics.cjs' },
  })
  fs.writeFileSync('diagnostics.cjs', "for (let i = 0; i < 14; i++) { console.log('root-stdout-' + i); console.error('root-stderr-' + i) }; process.exit(1)")
  for (const frozen of [false, true]) {
    const args = ['install', '--loglevel', level, frozen ? '--frozen-lockfile' : '--no-frozen-lockfile']
    if (reporter) args.push('--reporter', reporter)
    const result = execPnpmSync(args, { env: reporterEnv })
    expect(result.status).toBe(1)
    const output = result.stdout.toString() + result.stderr.toString()
    expect(output).toContain('postinstall$ node diagnostics.cjs')
    for (let index = 0; index < 14; index++) {
      expect(output).toContain(`root-stdout-${index}`)
      expect(output).toContain(`root-stderr-${index}`)
    }
    expect(output).toContain('Failed')
    expect(output).not.toContain('Progress:')
  }
})

test.each(['warn', 'error'])('dependency install failures are reported at %s on resolved and frozen paths', (level) => {
  prepare({ dependencies: { [failure]: '1.0.0' } })
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { [failure]: true }, optimisticRepeatInstall: false })
  for (const frozen of [false, true]) {
    if (frozen) execPnpmSync(['install', '--lockfile-only', '--ignore-scripts'], { expectSuccess: true })
    fs.rmSync('node_modules', { recursive: true, force: true })
    const result = execPnpmSync(['install', '--loglevel', level, frozen ? '--frozen-lockfile' : '--no-frozen-lockfile'], { env: reporterEnv })
    expect(result.status).toBe(1)
    const output = result.stdout.toString() + result.stderr.toString()
    expect(output).toContain('postinstall$ echo hello && echo world && exit 1')
    expect(output).toContain('postinstall: hello')
    expect(output).toContain('postinstall: world')
    expect(output).toContain('Failed')
  }
})

test.each(['warn', 'error'])('successful root and dependency scripts stay quiet at %s while still running', (level) => {
  prepare({ dependencies: { [success]: '1.0.0' }, scripts: { postinstall: 'node success.cjs' } })
  fs.writeFileSync('success.cjs', "require('fs').writeFileSync('root-built', 'yes'); console.log('successful-root-output')")
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { [success]: true }, optimisticRepeatInstall: false })
  for (const frozen of [false, true]) {
    fs.rmSync('node_modules', { recursive: true, force: true })
    fs.rmSync('root-built', { force: true })
    const result = execPnpmSync(['install', '--loglevel', level, frozen ? '--frozen-lockfile' : '--no-frozen-lockfile'], { env: reporterEnv, expectSuccess: true })
    expect(fs.readFileSync('root-built', 'utf8')).toBe('yes')
    expect(fs.existsSync(`node_modules/${success}/generated-by-install.js`)).toBe(true)
    expect(result.stdout.toString() + result.stderr.toString()).not.toMatch(/successful-root-output|install\$|install: Done/)
  }
})

test.each(['warn', 'error'])('optional script failure preserves successful install status at %s', (level) => {
  prepare({ optionalDependencies: { [failure]: '1.0.0' } })
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { [failure]: true }, optimisticRepeatInstall: false })
  for (const frozen of [false, true]) {
    fs.rmSync('node_modules', { recursive: true, force: true })
    const result = execPnpmSync(['install', '--loglevel', level, frozen ? '--frozen-lockfile' : '--no-frozen-lockfile'], { env: reporterEnv, expectSuccess: true })
    const output = result.stdout.toString() + result.stderr.toString()
    expect(output.includes('postinstall: hello')).toBe(level === 'warn')
    expect(output.includes('skipped as optional')).toBe(level === 'warn')
  }
})

test.each(['warn', 'error'])('ignored build warnings at %s preserve strict fatal enforcement', (level) => {
  prepare({ dependencies: { [success]: '1.0.0' } })
  writeYamlFileSync('pnpm-workspace.yaml', { strictDepBuilds: false, optimisticRepeatInstall: false })
  const result = execPnpmSync(['install', '--loglevel', level], { env: reporterEnv, expectSuccess: true })
  expect(result.stdout.toString().includes('Ignored build scripts')).toBe(level === 'warn')
  expect(fs.existsSync(`node_modules/${success}/generated-by-install.js`)).toBe(false)
  const strict = execPnpmSync(['install', '--loglevel', level, '--config.strict-dep-builds=true'], { env: reporterEnv })
  expect(strict.status).toBe(1)
  expect(strict.stdout.toString() + strict.stderr.toString()).toContain('Ignored build scripts')
})

test('explicit silent and NDJSON reporters retain their install contracts', () => {
  prepare({ scripts: { postinstall: 'node diagnostics.cjs' } })
  fs.writeFileSync('diagnostics.cjs', "console.log('root-reporter-output'); console.error('root-reporter-error'); process.exit(1)")
  const silent = execPnpmSync(['install', '--loglevel=warn', '--reporter=silent'], { env: reporterEnv })
  expect(silent.status).toBe(1)
  expect(silent.stdout.toString() + silent.stderr.toString()).toContain('root-reporter-output')
  expect(silent.stdout.toString() + silent.stderr.toString()).not.toContain('postinstall$')
  const silentLevel = execPnpmSync(['install', '--loglevel=silent'], { env: reporterEnv })
  expect(silentLevel.status).toBe(1)
  expect(silentLevel.stdout.toString() + silentLevel.stderr.toString()).toContain('root-reporter-output')
  expect(silentLevel.stdout.toString() + silentLevel.stderr.toString()).not.toContain('postinstall$')
  const ndjson = execPnpmSync(['install', '--loglevel=warn', '--reporter=ndjson'], { env: reporterEnv })
  expect(ndjson.status).toBe(1)
  expect(ndjson.stdout.toString()).toContain('root-reporter-output')
  expect(ndjson.stderr.toString()).toContain('root-reporter-error')
  expect(ndjson.stdout.toString()).not.toContain('postinstall$')
  const records: Array<{ name: string }> = ndjson.stdout.toString().trim().split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line))
  expect(records.some(record => record.name.startsWith('pnpm:'))).toBe(true)
})

test('dependency NDJSON output remains structured at quiet log levels', () => {
  prepare({ dependencies: { [failure]: '1.0.0' } })
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { [failure]: true } })
  const result = execPnpmSync(['install', '--loglevel=warn', '--reporter=ndjson'], { env: reporterEnv })
  expect(result.status).toBe(1)
  const records = result.stdout.toString().trim().split('\n').map(line => JSON.parse(line))
  expect(records).toContainEqual(expect.objectContaining({ name: 'pnpm:lifecycle', line: 'hello' }))
  expect(records).toContainEqual(expect.objectContaining({ name: 'pnpm:lifecycle', line: 'world' }))
  expect(records).toContainEqual(expect.objectContaining({ name: 'pnpm:lifecycle', exitCode: 1 }))
})

test.each(['warn', 'error'])('direct run still streams successful script output at %s', (level) => {
  prepare({ scripts: { test: 'node -e "console.log(\'direct-run-output\')"' } })
  const result = execPnpmSync(['--loglevel', level, 'run', 'test'], { env: reporterEnv, expectSuccess: true })
  expect(result.stdout.toString()).toContain('direct-run-output')
})

function parallelBuildScript (name: string): string {
  return `const fs = require('fs');
const root = ${JSON.stringify(process.cwd())};
const name = '${name}';
const other = '${name === 'failure' ? 'success' : 'failure'}';
fs.writeFileSync(root + '/ready-' + name, 'yes');
const deadline = Date.now() + 20000;
function finish () {
  if (!fs.existsSync(root + '/ready-' + other)) {
    if (Date.now() > deadline) process.exit(2);
    setImmediate(finish); return;
  }
  fs.writeFileSync(root + '/completed-' + name, 'yes');
  fs.writeFileSync('built', 'yes');
  console.log('parallel-' + name + '-stdout'); console.error('parallel-' + name + '-stderr');
  process.exit(${name === 'failure' ? 1 : 0});
}
finish();`
}

test.each(['warn', 'error'])('parallel dependency builds isolate failed and successful output at %s', (level) => {
  prepare({ optionalDependencies: { 'parallel-failure': 'file:failure', 'parallel-success': 'file:success' } })
  for (const name of ['failure', 'success']) {
    fs.mkdirSync(name)
    fs.writeFileSync(`${name}/package.json`, JSON.stringify({ name: `parallel-${name}`, version: '1.0.0', scripts: { postinstall: 'node build.cjs' } }))
    fs.writeFileSync(`${name}/build.cjs`, parallelBuildScript(name))
  }
  writeYamlFileSync('pnpm-workspace.yaml', { allowBuilds: { 'parallel-failure@file:failure': true, 'parallel-success@file:success': true } })
  const result = execPnpmSync(['install', '--loglevel', level, '--child-concurrency=2'], { env: reporterEnv, expectSuccess: true })
  const output = result.stdout.toString() + result.stderr.toString()
  for (const name of ['failure', 'success']) {
    if (!fs.existsSync(`completed-${name}`)) throw new Error(`${name} did not complete its concurrent build:\n${output}`)
    expect(fs.readFileSync(`completed-${name}`, 'utf8')).toBe('yes')
  }
  expect(fs.readFileSync('node_modules/parallel-success/built', 'utf8')).toBe('yes')
  expect(output.includes('parallel-failure-stdout')).toBe(level === 'warn')
  expect(output.includes('parallel-failure-stderr')).toBe(level === 'warn')
  expect(output).not.toContain('parallel-success-stdout')
  expect(output).not.toContain('parallel-success-stderr')
})
