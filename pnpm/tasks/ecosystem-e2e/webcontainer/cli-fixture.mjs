import { spawn } from 'node:child_process'
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'

if (process.argv[2] === '--matching-pin') await installWithMatchingPin(process.argv[3])
else if (process.argv[2] === '--approval') createApprovalFixture()
else if (process.argv[2] === '--frontend') createFrontendFixture()
else createWorkspaceFixture()

async function installWithMatchingPin (entry) {
  const { packageManager } = JSON.parse(readFileSync('package.json', 'utf8'))
  const version = packageManager.slice('pnpm@'.length)
  const integrity = `sha512-${Buffer.alloc(64).toString('base64')}`
  let metadataRequested = false
  // A matching pin records integrity even when the running build is unpublished.
  const server = createServer((request, response) => {
    if (request.url === '/pnpm') {
      metadataRequested = true
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({
        name: 'pnpm',
        'dist-tags': { latest: version },
        time: { [version]: '2020-01-01T00:00:00.000Z' },
        versions: {
          [version]: {
            name: 'pnpm', version,
            dist: { tarball: `https://registry.npmjs.org/pnpm/-/pnpm-${version}.tgz`, integrity },
          },
        },
      }))
      return
    }
    response.writeHead(404)
    response.end('Fixture package not found')
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  try {
    const child = spawn(process.execPath, [entry, 'install'], {
      stdio: 'inherit',
      env: { ...process.env, PNPM_CONFIG_REGISTRY: `http://127.0.0.1:${server.address().port}/` },
    })
    process.exitCode = await new Promise((resolve, reject) => {
      child.once('error', reject)
      child.once('exit', code => resolve(code ?? 1))
    })
  } finally {
    await new Promise(resolve => server.close(resolve))
  }
  if (process.exitCode !== 0) return
  assert.ok(metadataRequested, 'Matching pin did not request fixture metadata')
  const envLockfile = readFileSync('pnpm-lock.yaml', 'utf8').split('\n---\n')[0]
  assert.ok(envLockfile.includes(`    packageManagerDependencies:\n      pnpm:\n        specifier: ${version}\n        version: ${version}`), 'Matching pin was not recorded')
  assert.ok(envLockfile.includes(`  pnpm@${version}:\n    resolution: {integrity: ${integrity}}`), 'Fixture integrity was not recorded')
}

function createFrontendFixture () {
  const manifest = JSON.parse(readFileSync('package.json', 'utf8'))
  manifest.devDependencies = { vite: '7.3.1' }
  writeFileSync('package.json', JSON.stringify(manifest))
  appendFileSync('pnpm-workspace.yaml', '\noverrides:\n  esbuild: npm:esbuild-wasm@^0.27.0\n  rollup: npm:@rollup/wasm-node@^4.43.1\n')
  writeFileSync('index.html', '<div id="app"></div><script type="module" src="/main.js"></script>')
  writeFileSync('main.js', "document.querySelector('#app').textContent = 'pnpm WebContainer build'\n")
}

function createWorkspaceFixture () {
  const manifest = JSON.parse(readFileSync('package.json', 'utf8'))
  manifest.dependencies['@fixture/local'] = 'workspace:*'
  manifest.scripts = {
    postinstall: 'node -e "require(\'node:fs\').writeFileSync(\'lifecycle-ok\', \'yes\')"',
    verify: 'node -e "require(\'@fixture/local\'); console.log(\'wasm-script-ok\')"',
  }
  writeFileSync('package.json', JSON.stringify(manifest))
  writeFileSync('pnpm-workspace.yaml', "packages:\n  - packages/*\n")
  mkdirSync('packages/local', { recursive: true })
  writeFileSync('packages/local/package.json', JSON.stringify({
    name: '@fixture/local', version: '1.0.0', main: 'index.cjs',
    bin: { 'fixture-bin': 'bin.cjs' },
    scripts: { build: 'node -e "console.log(\'wasm-filter-ok\')"' },
    dependencies: { semver: '7.6.3' },
    peerDependencies: { chalk: '^4.0.0' },
  }))
  writeFileSync('packages/local/index.cjs', "module.exports = require('semver').valid('1.2.3')\n")
  writeFileSync('packages/local/bin.cjs', `#!/usr/bin/env node
require('node:assert/strict').deepEqual(process.argv.slice(2), ['a b', '$(false)', "'quoted'"])
console.log('wasm-bin-ok')
`)
}

function createApprovalFixture () {
  mkdirSync('pending-build', { recursive: true })
  writeFileSync('pending-build/package.json', JSON.stringify({
    name: 'fixture-needs-build', version: '1.0.0',
    scripts: { postinstall: 'node -e "require(\'node:fs\').writeFileSync(\'built\', \'yes\')"' },
  }))
  const manifest = JSON.parse(readFileSync('package.json', 'utf8'))
  manifest.dependencies['fixture-needs-build'] = 'file:./fixture-needs-build-1.0.0.tgz'
  writeFileSync('package.json', JSON.stringify(manifest))
  appendFileSync('pnpm-workspace.yaml', 'strictDepBuilds: false\n')
}
import assert from 'node:assert/strict'
