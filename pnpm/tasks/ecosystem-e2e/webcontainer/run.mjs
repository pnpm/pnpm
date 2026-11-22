// Installs a small project with this repo's pnpm bundle inside a StackBlitz
// WebContainer, booted in headless Chromium, and fails if any step exits
// non-zero. WebContainers run Node.js in the browser with their own `fs` and
// `node:sqlite`, which differ from Node.js in ways no local test reproduces.
//
// Usage: node run.mjs --pnpm-dir <dir>
// <dir> is a pnpm v11 package with a built dist/, such as an installed `pnpm@11`.
// --wasm-probe <file> runs the Rust WASI capability probe instead of pnpm.

import fs from 'node:fs'
import http from 'node:http'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs, stripVTControlCharacters } from 'node:util'

import { chromium } from 'playwright-core'

import { cleanCapabilitySteps, coreWorkflowSteps, inspectionWorkflowSteps, networkCapabilitySteps, packageInstallSteps, packageWorkflowSteps } from './cli-workflows.mjs'

const STEP_TIMEOUT_MS = 10 * 60_000

const PNPM = ['../pnpm/bin/pnpm.mjs']

const IS_ODD_ABSENT = `
const { dependencies } = require('./package.json')
if ('is-odd' in dependencies) throw new Error('is-odd is still in package.json')
let resolved
try {
  resolved = require.resolve('is-odd')
} catch {}
if (resolved) throw new Error('is-odd still resolves to ' + resolved)
`

const STEPS = [
  { name: 'install without a lockfile', command: 'node', args: [...PNPM, 'install'] },
  { name: 'load the installed packages', command: 'node', args: ['-e', "require('is-odd'); require('chalk')"] },
  { name: 'repeat install', command: 'node', args: [...PNPM, 'install'] },
  { name: 'remove node_modules and the lockfile', command: 'rm', args: ['-rf', 'node_modules', 'pnpm-lock.yaml'] },
  { name: 'install offline from the store', command: 'node', args: [...PNPM, 'install', '--offline'] },
  { name: 'add a dependency', command: 'node', args: [...PNPM, 'add', 'semver@7.6.3'] },
  { name: 'remove a dependency', command: 'node', args: [...PNPM, 'remove', 'is-odd'] },
  { name: 'check the dependency is gone', command: 'node', args: ['-e', IS_ODD_ABSENT] },
  { name: 'list dependencies', command: 'node', args: [...PNPM, 'list'], expectOutput: 'semver@7.6.3' },
  { name: 'load the added package', command: 'node', args: ['-e', "require('semver')"] },
]

const APP_MANIFEST = {
  name: 'webcontainer-e2e-app',
  private: true,
  dependencies: {
    chalk: '4.1.2',
    'is-odd': '3.0.1',
  },
}

const { values: opts } = parseArgs({
  options: {
    'pnpm-dir': { type: 'string' },
    'wasm-probe': { type: 'string' },
    'wasm-runtime': { type: 'string' },
    'wasm-cli': { type: 'string' },
    'wasm-inspection': { type: 'string' },
    'wasm-hoisted': { type: 'string' },
    'wasm-registry': { type: 'string' },
    'wasm-network': { type: 'string' },
    'wasm-global': { type: 'string' },
    'wasm-package': { type: 'string' },
    'wasm-package-registry': { type: 'string' },
    'wasm-package-frontend': { type: 'string' },
    'host-probe': { type: 'string' },
  },
})
if (Object.values(opts).some(value => value === '')) throw new Error('Probe artifact paths cannot be empty')
if ([opts['wasm-probe'], opts['wasm-runtime'], opts['wasm-cli'], opts['wasm-inspection'], opts['wasm-hoisted'], opts['wasm-registry'], opts['wasm-network'], opts['wasm-global'], opts['wasm-package'], opts['wasm-package-registry'], opts['wasm-package-frontend'], opts['host-probe']].filter(Boolean).length > 1) throw new Error('Select one probe mode')
const wasmCliArtifact = opts['wasm-cli'] ?? opts['wasm-inspection'] ?? opts['wasm-hoisted'] ?? opts['wasm-registry'] ?? opts['wasm-network'] ?? opts['wasm-global']
const packageArtifact = opts['wasm-package'] ?? opts['wasm-package-registry'] ?? opts['wasm-package-frontend']
const servedFiles = packageArtifact ? new Map([['pnpm-wasm.tgz', path.resolve(packageArtifact)]]) : wasmCliArtifact ? runtimeFiles().set('runtime/pnpm.wasm', path.resolve(wasmCliArtifact)) : opts['host-probe'] ? runtimeFiles().set('probe.mjs', path.resolve(opts['host-probe'])) : opts['wasm-runtime'] ? runtimeFiles(opts['wasm-runtime']) : opts['wasm-probe']
  ? new Map([
    ['probe.wasm', path.resolve(opts['wasm-probe'])],
    ['probe.mjs', fileURLToPath(new URL('wasm-probe.mjs', import.meta.url))],
  ])
  : pnpmPackageFiles(opts['pnpm-dir'])
if (opts['wasm-registry'] || opts['wasm-network'] || opts['wasm-cli'] || opts['wasm-inspection'] || opts['wasm-hoisted'] || packageArtifact) servedFiles.set('cli-fixture.mjs', fileURLToPath(new URL('./cli-fixture.mjs', import.meta.url)))
if (opts['wasm-network'] || opts['wasm-cli'] || packageArtifact) {
  const fixtures = fileURLToPath(new URL('../../../../pnpm/crates/network/tests/fixtures/', import.meta.url))
  servedFiles.set('network-ca.pem', path.join(fixtures, 'test-ca.pem'))
  servedFiles.set('network-client-key.pem', path.join(fixtures, 'test-client-pkcs1.key'))
}
if (opts['wasm-cli'] || packageArtifact) servedFiles.set('metadata-bench.mjs', fileURLToPath(new URL('./metadata-bench.mjs', import.meta.url)))
if (opts['wasm-cli'] || packageArtifact) servedFiles.set('webcontainer-commands.tsv', fileURLToPath(new URL('../../../wasm/webcontainer-commands.tsv', import.meta.url)))
if (opts['wasm-package']) servedFiles.set('process-tree-probe.mjs', fileURLToPath(new URL('./process-tree-probe.mjs', import.meta.url)))
for (const file of servedFiles.values()) fs.accessSync(file, fs.constants.R_OK)
const steps = opts['wasm-package'] ? [
  ...packageInstallSteps(),
  ...installedCliSteps('../installed/node_modules/.bin/pnpm'),
  { name: 'cancel an owned child process tree', command: 'node', args: ['../pnpm/process-tree-probe.mjs'], env: { PNPM_WASM_HOST_MODULE: '../installed/node_modules/@pnpm/wasm/dist/host/operations.mjs' }, expectOutput: 'Owned process tree cancellation passed' },
] : opts['wasm-package-registry'] ? [
  ...packageInstallSteps(),
  { name: 'registry workflows through the installed package', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--registry-workflow', '../installed/node_modules/.bin/pnpm'], expectOutput: 'Mock registry view, search, ping, audit, outdated, stars, staged publish, login, whoami, profile, token list, docs, publish, unpublish, access, owners, teams, tags, deprecation and logout passed' },
] : opts['wasm-package-frontend'] ? [
  ...packageInstallSteps().slice(0, 1),
  ...runInstalledEntry(frontendBuildSteps('../installed/node_modules/.bin/pnpm'), '../installed/node_modules/.bin/pnpm'),
] : opts['wasm-cli'] ? wasmCliSteps() : opts['wasm-registry'] ? [
  { name: 'prepare mounted executable launcher', command: 'node', args: ['-e', "require('node:fs').chmodSync('../pnpm/runtime/pnpm.mjs', 0o755)"] },
  { name: 'mock registry command workflows', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--registry-workflow', '../pnpm/runtime/pnpm.mjs'], expectOutput: 'Mock registry view, search, ping, audit, outdated, stars, staged publish, login, whoami, profile, token list, docs, publish, unpublish, access, owners, teams, tags, deprecation and logout passed' },
  ] : opts['wasm-hoisted'] ? [
    { name: 'prepare mounted executable launcher', command: 'node', args: ['-e', "require('node:fs').chmodSync('../pnpm/runtime/pnpm.mjs', 0o755)"] },
    { name: 'create hoisted workspace fixture', command: 'node', args: ['../pnpm/cli-fixture.mjs'] },
    { name: 'select the hoisted linker', command: 'node', args: ['-e', "require('node:fs').appendFileSync('pnpm-workspace.yaml', String.fromCharCode(10) + 'nodeLinker: hoisted' + String.fromCharCode(10))"] },
    { name: 'install with the hoisted linker', command: 'node', args: ['../pnpm/runtime/pnpm.mjs', 'install'] },
    { name: 'load workspace dependencies from hoisted node_modules', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('semver').valid('1.2.3'), '1.2.3'); require('@fixture/local')"] },
    { name: 'list hoisted workspace packages', command: 'node', args: ['../pnpm/runtime/pnpm.mjs', '--dir', 'packages/local', 'list'], expectOutput: 'semver@7.6.3' },
    { name: 'inspect hoisted dependency graph', command: 'node', args: ['../pnpm/runtime/pnpm.mjs', 'why', 'semver'], expectOutput: '@fixture/local' },
    { name: 'check hoisted peer dependencies', command: 'node', args: ['../pnpm/runtime/pnpm.mjs', 'peers', '--json'], expectOutput: 'missing' },
    { name: 'repeat hoisted install offline and frozen', command: 'node', args: ['../pnpm/runtime/pnpm.mjs', 'install', '--offline', '--frozen-lockfile'] },
  ] : opts['wasm-inspection'] ? [
    { name: 'prepare mounted executable launcher', command: 'node', args: ['-e', "require('node:fs').chmodSync('../pnpm/runtime/pnpm.mjs', 0o755)"] },
    { name: 'create inspection workspace fixture', command: 'node', args: ['../pnpm/cli-fixture.mjs'] },
    { name: 'install inspection workspace fixture', command: 'node', args: ['../pnpm/runtime/pnpm.mjs', 'install'] },
    ...inspectionWorkflowSteps('../pnpm/runtime/pnpm.mjs'),
    ...cleanCapabilitySteps('../pnpm/runtime/pnpm.mjs'),
  ] : opts['wasm-network'] ? [
    { name: 'prepare mounted executable launcher', command: 'node', args: ['-e', "require('node:fs').chmodSync('../pnpm/runtime/pnpm.mjs', 0o755)"] },
    ...networkCapabilitySteps('../pnpm/runtime/pnpm.mjs'),
  ] : opts['wasm-global'] ? [
    { name: 'prepare mounted executable launcher', command: 'node', args: ['-e', "require('node:fs').chmodSync('../pnpm/runtime/pnpm.mjs', 0o755)"] },
    { name: 'install a global package', command: 'node', args: ['../pnpm/runtime/pnpm.mjs', 'add', '--global', 'semver@7.6.3', '--config.global-bin-dir=/home/project/global/bin'], env: { PATH: '/home/project/global/bin:/home/.local/share/pnpm/bin:/usr/local/bin:/usr/bin:/bin' } },
    { name: 'list global packages', command: 'node', args: ['../pnpm/runtime/pnpm.mjs', 'list', '--global', '--depth', '0', '--parseable'], env: { PATH: '/home/project/global/bin:/home/.local/share/pnpm/bin:/usr/local/bin:/usr/bin:/bin' }, expectOutput: 'semver' },
  ] : opts['host-probe']
  ? [{ name: 'Node host capabilities', command: 'node', args: ['../pnpm/probe.mjs'] }]
  : opts['wasm-runtime']
  ? runtimeSteps()
  : opts['wasm-probe']
  ? [{ name: 'Rust WASI and Node host capabilities', command: 'node', args: ['../pnpm/probe.mjs'], expectOutput: 'pnpm-wasm-host-probe-ok' }]
  : STEPS
const webcontainerApiDir = path.dirname(fileURLToPath(import.meta.resolve('@webcontainer/api')))
const pagePath = fileURLToPath(new URL('page.html', import.meta.url))

const server = await startServer()
let browser
let failed = false
try {
  browser = await chromium.launch()
  const page = await browser.newPage()
  await page.exposeFunction('onOutput', chunk => process.stdout.write(chunk))
  page.on('pageerror', (err) => {
    console.error(`[page error] ${err.message}`)
  })
  await page.goto(`http://127.0.0.1:${server.address().port}/index.html`)
  await page.waitForFunction(() => window.ready)
  await withTimeout(page.evaluate(() => window.boot()), 'WebContainer boot')
  await withTimeout(page.evaluate((files) => window.mountPnpm(files), [...servedFiles.keys()]), 'mount files')
  await page.evaluate((manifest) => window.writeFile('app/package.json', manifest), JSON.stringify(APP_MANIFEST))

  for (const step of steps) {
    console.log(`\n=== ${step.name}: ${step.command} ${step.args.join(' ')}`)
    // eslint-disable-next-line no-await-in-loop
    const { exitCode, output } = await withTimeout(
      page.evaluate(({ command, args, env, input, cwd = 'app' }) => window.run(command, args, cwd, env, input), step),
      step.name
    )
    const text = stripVTControlCharacters(output).replaceAll('\r', '')
    if (exitCode !== (step.expectExitCode ?? 0)) {
      console.error(`FAIL: "${step.name}" exited with ${exitCode}`)
      failed = true
      break
    }
    if (step.expectOutput != null && !text.includes(step.expectOutput)) {
      console.error(`FAIL: "${step.name}" output does not contain "${step.expectOutput}"`)
      failed = true
      break
    }
    if (step.expectOutputSequence != null && !containsInOrder(text, step.expectOutputSequence)) {
      console.error(`FAIL: "${step.name}" output did not contain the expected sequence: ${step.expectOutputSequence.join(' -> ')}`)
      failed = true
      break
    }
    if (step.expectNoOutput != null && text.includes(step.expectNoOutput)) {
      console.error(`FAIL: "${step.name}" output unexpectedly contains "${step.expectNoOutput}"`)
      failed = true
      break
    }
  }
} finally {
  await browser?.close()
  server.close()
}
if (failed) process.exit(1)
console.log('\nAll WebContainer steps passed')

function wasmCliSteps (entry = '../pnpm/runtime/pnpm.mjs') {
  const steps = STEPS.map(step => step.args[0] === PNPM[0]
    ? { ...step, args: [entry, ...step.args.slice(1)] }
    : step)
  steps.splice(2, 0, {
    name: 'frozen lockfile install', command: 'node', args: [entry, 'install', '--frozen-lockfile'],
  })
  return [
    ...(opts['wasm-cli'] ? [{ name: 'prepare mounted executable launcher', command: 'node', args: ['-e', `require('node:fs').chmodSync(${JSON.stringify(entry)}, 0o755)`] }] : []),
    { name: 'Rust WASM CLI version', command: 'node', args: [entry, '--version'], expectOutput: '12.' },
    ...((opts['wasm-cli'] || opts['wasm-package']) ? [{ name: 'measure WebContainer filesystem metadata calls', command: 'node', args: ['../pnpm/metadata-bench.mjs'], expectOutput: 'webcontainer-metadata-bench-ok' }] : []),
    { name: 'Rust WASM CLI help', command: 'node', args: [entry, '--help'], expectOutput: 'Usage:' },
    { name: 'prepare command routing directory', command: 'node', args: ['-e', "require('node:fs').mkdirSync('command-routing', { recursive: true })"] },
    { name: 'route every classified command and alias', command: 'node', args: ['../../pnpm/cli-fixture.mjs', '--command-routing', opts['wasm-package'] ? '../../installed/node_modules/.bin/pnpm' : '../../pnpm/runtime/pnpm.mjs', '../../pnpm/webcontainer-commands.tsv'], cwd: 'app/command-routing', expectOutput: 'WebContainer command routing passed' },
    { name: 'isolated WASM store namespace', command: 'node', args: [entry, 'store', 'path'], expectOutput: 'v11-wasm' },
    { name: 'resolve metadata cache path', command: 'node', args: [entry, 'cache', 'path'], expectOutput: '/home/.cache/pnpm' },
    { name: 'setup fails without side effects', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--assert-no-effects', entry, 'setup', '--expect', 'This WebContainer runtime cannot install itself globally'], expectOutput: 'failed without changing project files' },
    { name: 'self-update fails without side effects', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--assert-no-effects', entry, 'self-update', '12.18.0', '--expect', 'Update this WebAssembly distribution by installing a newer version of @pnpm/wasm'], expectOutput: 'failed without changing project files' },
    { name: 'pack-app fails without side effects', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--assert-no-effects', entry, 'pack-app', '--expect', 'WebContainers cannot run that builder'], expectOutput: 'failed without changing project files' },
    { name: 'Cargo add fails without side effects', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--assert-no-effects', entry, '--cargo-add', '--expect', 'spawn cargo ENOENT'], expectOutput: 'failed without changing project files' },
    ...networkCapabilitySteps(entry),
    { name: 'registry commands use the WebContainer host', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--registry-workflow', entry], expectOutput: 'Mock registry view, search, ping, audit, outdated, stars, staged publish, login, whoami, profile, token list, docs, publish, unpublish, access, owners, teams, tags, deprecation and logout passed' },
    ...steps,
    { name: 'install alias i', command: 'node', args: [entry, 'i', '--offline'] },
    { name: 'list alias ls', command: 'node', args: [entry, 'ls'], expectOutput: 'semver@7.6.3' },
    ...coreWorkflowSteps(entry),
    { name: 'install a global package', command: 'node', args: [entry, 'add', '--global', 'semver@7.6.3', '--config.global-bin-dir=/home/project/global/bin'], env: { PATH: '/home/project/global/bin:/home/.local/share/pnpm/bin:/usr/local/bin:/usr/bin:/bin' } },
    { name: 'list global packages', command: 'node', args: [entry, 'list', '--global', '--depth', '0', '--parseable'], env: { PATH: '/home/project/global/bin:/home/.local/share/pnpm/bin:/usr/local/bin:/usr/bin:/bin' }, expectOutput: 'semver' },
    { name: 'execute a Node command', command: 'node', args: [entry, 'exec', 'node', '-e', "console.log('wasm-exec-ok')"], expectOutput: 'wasm-exec-ok' },
    { name: 'create workspace fixture', command: 'node', args: ['../pnpm/cli-fixture.mjs'] },
    { name: 'install workspace and run lifecycle', command: 'node', args: [entry, 'install'] },
    { name: 'verify lifecycle result', command: 'node', args: ['-e', "if (require('node:fs').readFileSync('lifecycle-ok', 'utf8') !== 'yes') throw new Error('lifecycle did not run')"] },
    { name: 'verify pnpmfile readPackage hook ran', command: 'node', args: ['-e', "require('node:assert/strict').ok(require('node:fs').readFileSync('pnpmfile-hook-ok', 'utf8').length > 0)"] },
    ...inspectionWorkflowSteps(entry),
    { name: 'resolve project root', command: 'node', args: [entry, 'root'], expectOutput: '/home/project/app/node_modules' },
    { name: 'resolve project prefix', command: 'node', args: [entry, 'prefix'], expectOutput: '/home/project/app' },
    { name: 'resolve project binary directory', command: 'node', args: [entry, 'bin'], expectOutput: '/home/project/app/node_modules/.bin' },
    { name: 'verify store package integrity', command: 'node', args: [entry, 'store', 'status'], expectOutput: 'Packages in the store are untouched' },
    { name: 'store status detects modified package content', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--store-status-mutation', entry], expectOutput: 'store status detected a modified installed package' },
    { name: 'execute workspace bin', command: 'node', args: [entry, 'exec', 'fixture-bin', 'a b', '$(false)', "'quoted'"], expectOutput: 'wasm-bin-ok' },
    { name: 'run project script', command: 'node', args: [entry, 'run', 'verify'], expectOutput: 'wasm-script-ok' },
    { name: 'run-script alias', command: 'node', args: [entry, 'run-script', 'verify'], expectOutput: 'wasm-script-ok' },
    { name: 'dispatch an external package script', command: 'node', args: [entry, 'wasm-external'], expectOutput: 'wasm-external-script-ok' },
    { name: 'run an overriding script for a built-in command', command: 'node', args: [entry, '--dir', 'packages/local', 'rebuild'], expectOutput: 'wasm-rebuild-script-override' },
    { name: 'force a built-in through pnpm pm', command: 'node', args: [entry, '--dir', 'packages/local', 'pm', 'rebuild'], expectNoOutput: 'wasm-rebuild-script-override' },
    { name: 'run filtered workspace script', command: 'node', args: [entry, '--filter', '@fixture/local', 'run', 'build'], expectOutput: 'wasm-filter-ok' },
    { name: 'run scripts recursively', command: 'node', args: [entry, '-r', 'run', 'build'], expectOutput: 'wasm-filter-ok' },
    { name: 'recursive alias m', command: 'node', args: [entry, 'm', 'run', 'build'], expectOutput: 'wasm-filter-ok' },
    { name: 'recursive alias multi', command: 'node', args: [entry, 'multi', 'run', 'build'], expectOutput: 'wasm-filter-ok' },
    { name: 'install frozen workspace offline', command: 'node', args: [entry, 'install', '--offline', '--frozen-lockfile'] },
    { name: 'create build approval fixture', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--approval'] },
    { name: 'pack dependency with a lifecycle script', command: 'npm', args: ['pack', './pending-build', '--ignore-scripts'] },
    { name: 'install without approving dependency build', command: 'node', args: [entry, 'install'] },
    { name: 'verify dependency build is blocked', command: 'node', args: ['-e', "if (require('node:fs').existsSync('node_modules/fixture-needs-build/built')) throw new Error('unapproved build ran')"] },
    {
      name: 'approve dependency build interactively', command: 'node', args: [entry, 'approve-builds'],
      input: [{ after: 'Choose which packages to build', text: ' \r' }, { after: 'Build fixture-needs-build', text: 'y\r' }],
    },
    { name: 'verify approved dependency build', command: 'node', args: ['-e', "if (require('node:fs').readFileSync('node_modules/fixture-needs-build/built', 'utf8') !== 'yes') throw new Error('approved build did not run')"] },
    ...packageWorkflowSteps(entry),
    // --wasm-package-frontend runs these in a fresh session; see the ecosystem-e2e README.
    ...(opts['wasm-package'] ? [] : frontendBuildSteps(entry)),
    { name: 'pin the running WASM version', command: 'node', args: ['-e', `const fs = require('node:fs'); const manifest = require('./package.json'); manifest.packageManager = 'pnpm@' + require('node:child_process').execFileSync('node', [${JSON.stringify(entry)}, '--version'], { encoding: 'utf8' }).trim(); fs.writeFileSync('package.json', JSON.stringify(manifest))`] },
    { name: 'install with matching package manager pin', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--matching-pin', entry] },
    { name: 'pin a different pnpm release', command: 'node', args: ['-e', "const fs = require('node:fs'); const manifest = require('./package.json'); manifest.packageManager = 'pnpm@12.8.1'; fs.writeFileSync('package.json', JSON.stringify(manifest))"] },
    { name: 'reject native version switching', command: 'node', args: [entry, 'install'], expectExitCode: 1, expectOutput: 'ERR_PNPM_UNSUPPORTED_RUNTIME' },
    { name: 'use installed WASM version explicitly', command: 'node', args: [entry, 'install'], env: { npm_config_manage_package_manager_versions: 'false' } },
    ...cleanCapabilitySteps(entry),
  ]
}

function frontendBuildSteps (entry) {
  return [
    { name: 'create frontend fixture', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--frontend'] },
    { name: 'install frontend toolchain', command: 'node', args: [entry, 'install'] },
    { name: 'build a Vite app', command: 'node', args: [entry, 'exec', 'vite', 'build'] },
    { name: 'verify frontend build', command: 'node', args: ['-e', "if (!require('node:fs').existsSync('dist/index.html')) throw new Error('Vite build output is missing')"] },
  ]
}

function installedCliSteps (entry) {
  return runInstalledEntry(wasmCliSteps(entry), entry)
}

function runInstalledEntry (steps, entry) {
  return steps.map(step => step.command === 'node' && step.args[0] === entry
    ? { ...step, command: entry, args: step.args.slice(1) }
    : step)
}

function runtimeSteps () {
  const args = ['../pnpm/runtime/run.mjs', '../pnpm/runtime/probe.wasm']
  return [
    { name: 'guest panic cleanup', command: 'node', args, expectExitCode: 1, expectOutput: 'PROBE_DIR must name a writable directory' },
    { name: 'Rust threaded runtime and host services', command: 'node', args, env: { PROBE_DIR: '/tmp' }, expectOutput: 'pnpm-wasm-runtime-probe-ok' },
    { name: 'SQLite concurrent access and crash recovery', command: 'node', args: ['../pnpm/runtime/store-probe.mjs'], env: { PNPM_STORE_PROBE_WASM: '../pnpm/runtime/probe.wasm', PNPM_STORE_PROBE_RUNNER: '../pnpm/runtime/run.mjs' }, expectOutput: 'pnpm-wasm-store-probe-ok' },
    { name: 'exit from a Rust worker thread', command: 'node', args, env: { PROBE_THREAD_EXIT: '1' }, expectExitCode: 23 },
  ]
}

function runtimeFiles (artifact) {
  const directory = fileURLToPath(new URL('../../../wasm/', import.meta.url))
  const files = new Map()
  if (artifact) {
    files.set('runtime/probe.wasm', path.resolve(artifact))
    files.set('runtime/store-probe.mjs', fileURLToPath(new URL('./runtime-probe/store-probe.mjs', import.meta.url)))
  }
  addDirectory(files, directory, 'runtime', true)
  const packages = new Map()
  addDependencies(files, directory, packages)
  return files
}

function addDirectory (files, directory, prefix, source = false) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name.endsWith('.map') || entry.name.endsWith('.test.mjs')) continue
    const file = path.join(directory, entry.name)
    const destination = `${prefix}/${entry.name}`
    if (entry.isDirectory()) addDirectory(files, file, destination, source)
    else if (entry.isFile() && (!source || entry.name.endsWith('.mjs') || entry.name === 'package.json')) files.set(destination, file)
  }
}

function addDependencies (files, directory, packages) {
  const manifest = JSON.parse(fs.readFileSync(path.join(directory, 'package.json'), 'utf8'))
  const require = createRequire(path.join(directory, 'package.json'))
  for (const name of Object.keys(manifest.dependencies ?? {})) {
    let dependency = path.dirname(require.resolve(name))
    while (!fs.existsSync(path.join(dependency, 'package.json'))) {
      const parent = path.dirname(dependency)
      if (parent === dependency) throw new Error(`Cannot locate package.json for ${name}`)
      dependency = parent
    }
    if (packages.has(name)) {
      if (packages.get(name) !== dependency) throw new Error(`Runtime fixture needs conflicting versions of ${name}`)
      continue
    }
    packages.set(name, dependency)
    addDirectory(files, dependency, `runtime/node_modules/${name}`)
    addDependencies(files, dependency, packages)
  }
}

function pnpmPackageFiles (pnpmDirOption) {
  if (pnpmDirOption == null) throw new Error('Pass --pnpm-dir or a probe mode')
  const pnpmDir = path.resolve(pnpmDirOption)
  return new Map(listPnpmPackageFiles(pnpmDir).map(file => [file, path.join(pnpmDir, file)]))
}

/**
 * The files the published pnpm package contains: package.json, bin/, and
 * dist/ without source maps, as the `files` field of the pnpm v11 package selects.
 */
function listPnpmPackageFiles (dir) {
  if (!fs.existsSync(path.join(dir, 'dist/pnpm.mjs'))) {
    throw new Error(`${dir}/dist/pnpm.mjs is missing`)
  }
  const files = ['package.json']
  for (const root of ['bin', 'dist']) {
    for (const entry of fs.readdirSync(path.join(dir, root), { recursive: true, withFileTypes: true })) {
      if (!entry.isFile() || entry.name.endsWith('.map')) continue
      files.push(path.relative(dir, path.join(entry.parentPath, entry.name)).split(path.sep).join('/'))
    }
  }
  return files
}

/**
 * Serves the page, the WebContainer API, and the pnpm package files on an
 * ephemeral 127.0.0.1 port. Resolves with the listening server, or rejects if
 * it cannot listen.
 *
 * WebContainers need SharedArrayBuffer, which browsers only expose to
 * cross-origin isolated pages, hence the COOP and COEP headers.
 */
function startServer () {
  const httpServer = http.createServer((req, res) => {
    res.setHeader('Cross-Origin-Opener-Policy', 'same-origin')
    res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp')
    const url = decodeURIComponent(new URL(req.url, 'http://localhost').pathname)
    const file = resolveServedFile(url)
    if (file == null) {
      res.statusCode = 404
      res.end()
      return
    }
    res.setHeader('Content-Type', contentType(file))
    fs.createReadStream(file).pipe(res)
  })
  return new Promise((resolve, reject) => {
    httpServer.once('error', reject)
    httpServer.listen(0, '127.0.0.1', () => {
      resolve(httpServer)
    })
  })
}

function resolveServedFile (url) {
  if (url === '/index.html') return pagePath
  if (url.startsWith('/pnpm/')) {
    const relative = url.slice('/pnpm/'.length)
    return servedFiles.get(relative)
  }
  if (url.startsWith('/webcontainer-api/')) {
    const file = path.join(webcontainerApiDir, url.slice('/webcontainer-api/'.length))
    return file.startsWith(webcontainerApiDir + path.sep) && fs.existsSync(file) ? file : undefined
  }
  return undefined
}

function contentType (file) {
  if (file.endsWith('.html')) return 'text/html'
  if (file.endsWith('.js') || file.endsWith('.mjs') || file.endsWith('.cjs')) return 'text/javascript'
  return 'application/octet-stream'
}

function withTimeout (promise, name) {
  let timer
  const timeout = new Promise((_resolve, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`"${name}" did not finish within ${STEP_TIMEOUT_MS / 60_000} minutes`))
    }, STEP_TIMEOUT_MS)
  })
  return Promise.race([promise, timeout]).finally(() => {
    clearTimeout(timer)
  })
}

function containsInOrder (text, parts) {
  let position = 0
  for (const part of parts) {
    const nextPosition = text.indexOf(part, position)
    if (nextPosition === -1) return false
    position = nextPosition + part.length
  }
  return true
}
