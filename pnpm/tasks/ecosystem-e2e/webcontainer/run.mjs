// Installs a small project with this repo's pnpm bundle inside a StackBlitz
// WebContainer, booted in headless Chromium, and fails if any step exits
// non-zero. WebContainers run Node.js in the browser with their own `fs` and
// `node:sqlite`, which differ from Node.js in ways no local test reproduces.
//
// Usage: node run.mjs [--pnpm-dir <dir>]
// <dir> is the pnpm package with a built dist/, pnpm11/pnpm by default.

import fs from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { parseArgs, stripVTControlCharacters } from 'node:util'

import { chromium } from 'playwright-core'

const STEP_TIMEOUT_MS = 10 * 60_000

const PNPM = ['../pnpm/bin/pnpm.mjs']

const STEPS = [
  { name: 'install without a lockfile', command: 'node', args: [...PNPM, 'install'] },
  { name: 'load the installed packages', command: 'node', args: ['-e', "require('is-odd'); require('chalk')"] },
  { name: 'repeat install', command: 'node', args: [...PNPM, 'install'] },
  { name: 'remove node_modules and the lockfile', command: 'rm', args: ['-rf', 'node_modules', 'pnpm-lock.yaml'] },
  { name: 'install from the store', command: 'node', args: [...PNPM, 'install'] },
  { name: 'add a dependency', command: 'node', args: [...PNPM, 'add', 'semver@7.6.3'] },
  { name: 'remove a dependency', command: 'node', args: [...PNPM, 'remove', 'is-odd'] },
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
  },
})
const pnpmDir = path.resolve(opts['pnpm-dir'])
const pnpmFiles = listPnpmPackageFiles(pnpmDir)
const webcontainerApiDir = path.dirname(fileURLToPath(import.meta.resolve('@webcontainer/api')))
const pagePath = fileURLToPath(new URL('page.html', import.meta.url))

const server = await startServer()
const browser = await chromium.launch()
let failed = false
try {
  const page = await browser.newPage()
  page.on('pageerror', (err) => {
    console.error(`[page error] ${err.message}`)
  })
  await page.goto(`http://127.0.0.1:${server.address().port}/index.html`)
  await page.waitForFunction(() => window.ready)
  await page.evaluate(() => window.boot())
  await page.evaluate((files) => window.mountPnpm(files), pnpmFiles)
  await page.evaluate((manifest) => window.writeFile('app/package.json', manifest), JSON.stringify(APP_MANIFEST))

  for (const step of STEPS) {
    console.log(`\n=== ${step.name}: ${step.command} ${step.args.join(' ')}`)
    // eslint-disable-next-line no-await-in-loop
    const { exitCode, output } = await withTimeout(
      page.evaluate(({ command, args }) => window.run(command, args, 'app'), step),
      step.name
    )
    const text = stripVTControlCharacters(output).replaceAll('\r', '')
    console.log(text)
    if (exitCode !== 0) {
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
  await browser.close()
  server.close()
}
if (failed) process.exit(1)
console.log('\nAll WebContainer steps passed')

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
 * WebContainers need SharedArrayBuffer, which browsers only expose to
 * cross-origin isolated pages, hence the COOP and COEP headers.
 */
function startServer () {
  const servedPnpmFiles = new Set(pnpmFiles)
  const httpServer = http.createServer((req, res) => {
    res.setHeader('Cross-Origin-Opener-Policy', 'same-origin')
    res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp')
    const url = decodeURIComponent(new URL(req.url, 'http://localhost').pathname)
    const file = resolveServedFile(url, servedPnpmFiles)
    if (file == null) {
      res.statusCode = 404
      res.end()
      return
    }
    res.setHeader('Content-Type', contentType(file))
    fs.createReadStream(file).pipe(res)
  })
  return new Promise((resolve) => {
    httpServer.listen(0, '127.0.0.1', () => {
      resolve(httpServer)
    })
  })
}

function resolveServedFile (url, servedPnpmFiles) {
  if (url === '/index.html') return pagePath
  if (url.startsWith('/pnpm/')) {
    const relative = url.slice('/pnpm/'.length)
    return servedPnpmFiles.has(relative) ? path.join(pnpmDir, relative) : undefined
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
