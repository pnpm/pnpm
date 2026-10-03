// Installs a small project with this repo's pnpm bundle inside a StackBlitz
// WebContainer, booted in headless Chromium, and fails if any step exits
// non-zero. WebContainers run Node.js in the browser with their own `fs` and
// `node:sqlite`, which differ from Node.js in ways no local test reproduces.
//
// Usage: node run.mjs [--pnpm-dir <dir>]
// <dir> is the pnpm package with a built dist/, pnpm11/pnpm by default.
// --wasm-probe <file> runs the Rust WASI capability probe instead of pnpm.

import fs from 'node:fs'
import http from 'node:http'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs, stripVTControlCharacters } from 'node:util'

import { chromium } from 'playwright-core'

import { coreWorkflowSteps, packageInstallSteps, packageWorkflowSteps } from './cli-workflows.mjs'

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
    'pnpm-dir': { type: 'string', default: fileURLToPath(new URL('../../../../pnpm11/pnpm', import.meta.url)) },
    'wasm-probe': { type: 'string' },
    'wasm-runtime': { type: 'string' },
    'wasm-cli': { type: 'string' },
    'wasm-package': { type: 'string' },
    'host-probe': { type: 'string' },
  },
})
const pnpmDir = path.resolve(opts['pnpm-dir'])
if ([opts['wasm-probe'], opts['wasm-runtime'], opts['wasm-cli'], opts['wasm-package'], opts['host-probe']].filter(Boolean).length > 1) throw new Error('Select one probe mode')
const servedFiles = opts['wasm-package'] ? new Map([['pnpm-wasm.tgz', path.resolve(opts['wasm-package'])]]) : opts['wasm-cli'] ? runtimeFiles().set('runtime/pnpm.wasm', path.resolve(opts['wasm-cli'])) : opts['host-probe'] ? runtimeFiles().set('probe.mjs', path.resolve(opts['host-probe'])) : opts['wasm-runtime'] ? runtimeFiles(opts['wasm-runtime']) : opts['wasm-probe']
  ? new Map([
    ['probe.wasm', path.resolve(opts['wasm-probe'])],
    ['probe.mjs', fileURLToPath(new URL('wasm-probe.mjs', import.meta.url))],
  ])
  : new Map(listPnpmPackageFiles(pnpmDir).map(file => [file, path.join(pnpmDir, file)]))
if (opts['wasm-cli'] || opts['wasm-package']) servedFiles.set('cli-fixture.mjs', fileURLToPath(new URL('./cli-fixture.mjs', import.meta.url)))
for (const file of servedFiles.values()) fs.accessSync(file, fs.constants.R_OK)
const steps = opts['wasm-package'] ? [
  ...packageInstallSteps(),
  ...installedCliSteps('../installed/node_modules/.bin/pnpm'),
] : opts['wasm-cli'] ? wasmCliSteps() : opts['host-probe']
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
    { name: 'Rust WASM CLI help', command: 'node', args: [entry, '--help'], expectOutput: 'Usage:' },
    { name: 'isolated WASM store namespace', command: 'node', args: [entry, 'store', 'path'], expectOutput: 'v11-wasm' },
    ...steps,
    ...coreWorkflowSteps(entry),
    { name: 'execute a Node command', command: 'node', args: [entry, 'exec', 'node', '-e', "require('semver'); console.log('wasm-exec-ok')"], expectOutput: 'wasm-exec-ok' },
    { name: 'create workspace fixture', command: 'node', args: ['../pnpm/cli-fixture.mjs'] },
    { name: 'install workspace and run lifecycle', command: 'node', args: [entry, 'install'] },
    { name: 'verify lifecycle result', command: 'node', args: ['-e', "if (require('node:fs').readFileSync('lifecycle-ok', 'utf8') !== 'yes') throw new Error('lifecycle did not run')"] },
    { name: 'execute workspace bin', command: 'node', args: [entry, 'exec', 'fixture-bin', 'a b', '$(false)', "'quoted'"], expectOutput: 'wasm-bin-ok' },
    { name: 'run project script', command: 'node', args: [entry, 'run', 'verify'], expectOutput: 'wasm-script-ok' },
    { name: 'run filtered workspace script', command: 'node', args: [entry, '--filter', '@fixture/local', 'run', 'build'], expectOutput: 'wasm-filter-ok' },
    { name: 'install frozen workspace offline', command: 'node', args: [entry, 'install', '--offline', '--frozen-lockfile'] },
    { name: 'create build approval fixture', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--approval'] },
    { name: 'pack dependency with a lifecycle script', command: 'npm', args: ['pack', './pending-build', '--ignore-scripts'] },
    { name: 'install without approving dependency build', command: 'node', args: [entry, 'install'] },
    { name: 'verify dependency build is blocked', command: 'node', args: ['-e', "if (require('node:fs').existsSync('node_modules/fixture-needs-build/built')) throw new Error('unapproved build ran')"] },
    {
      name: 'approve dependency build interactively', command: 'node', args: [entry, 'approve-builds'],
      input: [{ after: 'Choose which packages to build', text: ' \r' }, { after: 'Do you approve?', text: 'y\r' }],
    },
    { name: 'verify approved dependency build', command: 'node', args: ['-e', "if (require('node:fs').readFileSync('node_modules/fixture-needs-build/built', 'utf8') !== 'yes') throw new Error('approved build did not run')"] },
    ...packageWorkflowSteps(entry),
    { name: 'create frontend fixture', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--frontend'] },
    { name: 'install frontend toolchain', command: 'node', args: [entry, 'install'] },
    { name: 'build a Vite app', command: 'node', args: [entry, 'exec', 'vite', 'build'] },
    { name: 'verify frontend build', command: 'node', args: ['-e', "if (!require('node:fs').existsSync('dist/index.html')) throw new Error('Vite build output is missing')"] },
    { name: 'pin the running WASM version', command: 'node', args: ['-e', `const fs = require('node:fs'); const manifest = require('./package.json'); manifest.packageManager = 'pnpm@' + require('node:child_process').execFileSync('node', [${JSON.stringify(entry)}, '--version'], { encoding: 'utf8' }).trim(); fs.writeFileSync('package.json', JSON.stringify(manifest))`] },
    { name: 'install with matching package manager pin', command: 'node', args: [entry, 'install'] },
    { name: 'pin a different pnpm release', command: 'node', args: ['-e', "const fs = require('node:fs'); const manifest = require('./package.json'); manifest.packageManager = 'pnpm@12.8.1'; fs.writeFileSync('package.json', JSON.stringify(manifest))"] },
    { name: 'reject native version switching', command: 'node', args: [entry, 'install'], expectExitCode: 1, expectOutput: 'ERR_PNPM_UNSUPPORTED_RUNTIME' },
    { name: 'use installed WASM version explicitly', command: 'node', args: [entry, 'install'], env: { npm_config_manage_package_manager_versions: 'false' } },
  ]
}

function installedCliSteps (entry) {
  return wasmCliSteps(entry).map(step => step.command === 'node' && step.args[0] === entry
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

/**
 * The files the published pnpm package contains: package.json, bin/, and
 * dist/ without source maps, as the `files` field of pnpm11/pnpm selects.
 */
function listPnpmPackageFiles (dir) {
  if (!fs.existsSync(path.join(dir, 'dist/pnpm.mjs'))) {
    throw new Error(`${dir}/dist/pnpm.mjs is missing. Build the bundle with: pnpm --filter pnpm run compile`)
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
