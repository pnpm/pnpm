import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'

if (process.argv[2] === '--approval') createApprovalFixture()
else if (process.argv[2] === '--frontend') createFrontendFixture()
else createWorkspaceFixture()

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
