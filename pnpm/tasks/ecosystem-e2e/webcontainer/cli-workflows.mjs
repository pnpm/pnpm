export function coreWorkflowSteps (entry) {
  return [
    { name: 'update a dependency', command: 'node', args: [entry, 'update', 'semver@7.7.2'] },
    { name: 'verify updated dependency', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('semver/package.json').version, '7.7.2')"] },
    { name: 'make the lockfile outdated', command: 'node', args: ['-e', "const fs = require('node:fs'); const manifest = require('./package.json'); manifest.dependencies.semver = '7.6.3'; fs.writeFileSync('package.json', JSON.stringify(manifest))"] },
    { name: 'reject an outdated frozen lockfile', command: 'node', args: [entry, 'install', '--frozen-lockfile'], expectExitCode: 1, expectOutput: 'ERR_PNPM_OUTDATED_LOCKFILE' },
    { name: 'update only the lockfile', command: 'node', args: [entry, 'install', '--lockfile-only'] },
    { name: 'lockfile-only preserves installed dependencies', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('semver/package.json').version, '7.7.2')"] },
    { name: 'remove installed dependencies', command: 'rm', args: ['-rf', 'node_modules'] },
    { name: 'recreate dependencies from frozen lockfile offline', command: 'node', args: [entry, 'install', '--offline', '--frozen-lockfile'] },
    { name: 'verify restored dependency version', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('semver/package.json').version, '7.6.3')"] },
    { name: 'set project configuration', command: 'node', args: [entry, 'config', 'set', 'fetch-retries', '3', '--location', 'project'] },
    { name: 'read project configuration', command: 'node', args: [entry, 'config', 'get', 'fetch-retries'], expectOutput: '3' },
    { name: 'delete project configuration', command: 'node', args: [entry, 'config', 'delete', 'fetch-retries', '--location', 'project'] },
    { name: 'report a missing script', command: 'node', args: [entry, 'run', 'missing-script'], expectExitCode: 1, expectOutput: 'ERR_PNPM_NO_SCRIPT' },
    { name: 'propagate command failure', command: 'node', args: [entry, 'exec', 'node', '-e', 'process.exitCode = 17'], expectExitCode: 17 },
    { name: 'execute a registry package with dlx', command: 'node', args: [entry, 'dlx', 'semver@7.6.3', '1.2.3'], expectOutput: '1.2.3' },
    { name: 'create a frontend project', command: 'node', args: [entry, 'create', 'vite@7.1.2', 'created-app', '--template', 'vanilla'] },
    { name: 'verify generated project', command: 'node', args: ['-e', "const assert = require('node:assert/strict'); assert.equal(require('./created-app/package.json').name, 'created-app'); assert.ok(require('node:fs').existsSync('created-app/index.html'))"] },
  ]
}

export function packageWorkflowSteps (entry) {
  return [
    { name: 'remove approved build output', command: 'node', args: ['-e', "require('node:fs').unlinkSync('node_modules/fixture-needs-build/built')"] },
    { name: 'rebuild an approved dependency', command: 'node', args: [entry, 'rebuild', 'fixture-needs-build'] },
    { name: 'verify rebuilt dependency', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('node:fs').readFileSync('node_modules/fixture-needs-build/built', 'utf8'), 'yes')"] },
    { name: 'pack workspace package', command: 'node', args: [entry, '--dir', 'packages/local', 'pack', '--pack-destination', '../../packed'] },
    { name: 'verify packed package', command: 'node', args: ['-e', "require('node:assert/strict').ok(require('node:fs').statSync('packed/fixture-local-1.0.0.tgz').size > 0)"] },
  ]
}

export function packageInstallSteps () {
  return [
    { name: 'install @pnpm/wasm with npm', command: 'npm', args: ['install', '--prefix', '../installed', '../pnpm/pnpm-wasm.tgz'] },
    { name: 'installed package is @pnpm/wasm', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('../installed/node_modules/@pnpm/wasm/package.json').name, '@pnpm/wasm')"] },
    { name: 'installed package runs WASM', command: '../installed/node_modules/.bin/pnpm', args: ['store', 'path'], expectOutput: 'v11-wasm' },
    { name: 'npx dispatches the installed package', command: 'npx', args: ['--no-install', 'pnpm', '--version'], cwd: 'installed', expectOutput: '12.' },
    ...aliasSteps('../installed/node_modules/.bin'),
    { name: 'install @pnpm/wasm globally', command: 'npm', args: ['install', '--global', '--prefix', '../global', '../pnpm/pnpm-wasm.tgz'] },
    { name: 'global pnpm runs WASM on PATH', command: 'node', args: ['-e', "const env = { ...process.env, PATH: require('node:path').resolve('../global/bin') + ':' + process.env.PATH }; process.stdout.write(require('node:child_process').execFileSync('pnpm', ['store', 'path'], { env, encoding: 'utf8' }))"], expectOutput: 'v11-wasm' },
    ...aliasSteps('../global/bin'),
  ]
}

function aliasSteps (directory) {
  return [
    { name: `pn alias from ${directory}`, command: `${directory}/pn`, args: ['--version'], expectOutput: '12.' },
    { name: `pnpx alias from ${directory}`, command: `${directory}/pnpx`, args: ['semver@7.6.3', '2.3.4'], expectOutput: '2.3.4' },
    { name: `pnx alias from ${directory}`, command: `${directory}/pnx`, args: ['semver@7.6.3', '3.4.5'], expectOutput: '3.4.5' },
  ]
}
