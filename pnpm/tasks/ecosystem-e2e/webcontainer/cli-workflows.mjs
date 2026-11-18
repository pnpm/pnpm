export function coreWorkflowSteps (entry) {
  const removeAliasSteps = ['rm', 'un', 'uni', 'uninstall'].flatMap(alias => [
    { name: `add dependency before ${alias}`, command: 'node', args: [entry, 'add', 'semver@7.6.3'] },
    { name: `remove dependency with ${alias}`, command: 'node', args: [entry, alias, 'semver'] },
  ])
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
    { name: 'list configuration', command: 'node', args: [entry, 'config', 'list'], expectOutput: 'registry' },
    { name: 'report a missing script', command: 'node', args: [entry, 'run', 'missing-script'], expectExitCode: 1, expectOutput: 'ERR_PNPM_NO_SCRIPT' },
    { name: 'propagate command failure', command: 'node', args: [entry, 'exec', 'node', '-e', 'process.exitCode = 17'], expectExitCode: 17 },
    { name: 'execute a registry package with dlx', command: 'node', args: [entry, 'dlx', 'semver@7.6.3', '1.2.3'], expectOutput: '1.2.3' },
    { name: 'create a frontend project', command: 'node', args: [entry, 'create', 'vite@7.1.2', 'created-app', '--template', 'vanilla'] },
    { name: 'verify generated project', command: 'node', args: ['-e', "const assert = require('node:assert/strict'); assert.equal(require('./created-app/package.json').name, 'created-app'); assert.ok(require('node:fs').existsSync('created-app/index.html'))"] },
    ...removeAliasSteps,
  ]
}

export function inspectionWorkflowSteps (entry) {
  return [
    { name: 'show dependency dependents', command: 'node', args: [entry, 'why', 'semver'], expectOutput: '@fixture/local' },
    { name: 'require a package name for why', command: 'node', args: [entry, 'why'], expectExitCode: 1, expectOutput: 'ERR_PNPM_MISSING_PACKAGE_NAME' },
    { name: 'check workspace peer dependencies', command: 'node', args: [entry, 'peers', '--json'], expectOutput: 'missing' },
    { name: 'list package permissions', command: 'node', args: [entry, 'permissions', 'list'] },
    { name: 'list package permissions with ls alias', command: 'node', args: [entry, 'permissions', 'ls'] },
    { name: 'approve when no package permissions are pending', command: 'node', args: [entry, 'permissions', 'approve'], expectOutput: 'There are no packages awaiting approval' },
    { name: 'list ignored build scripts', command: 'node', args: [entry, 'ignored-builds'] },
    { name: 'list installed package licenses', command: 'node', args: [entry, 'licenses', '--json', 'list'], expectOutput: 'MIT' },
    { name: 'licenses alias licences', command: 'node', args: [entry, 'licences', '--json', 'list'], expectOutput: 'MIT' },
    { name: 'report package funding', command: 'node', args: [entry, 'fund'] },
    { name: 'generate CycloneDX SBOM from the lockfile', command: 'node', args: [entry, 'sbom', '--sbom-format', 'cyclonedx', '--lockfile-only'], expectOutput: 'CycloneDX' },
    { name: 'create an isolated init directory', command: 'node', args: ['-e', "require('node:fs').mkdirSync('init-audit')"] },
    { name: 'initialize a bare project without registry access', command: 'node', args: [entry, '--dir', 'init-audit', 'init', '--bare', '--no-init-package-manager'] },
    { name: 'verify initialized package manifest', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('./init-audit/package.json').type, 'module')"] },
    { name: 'reject init in an existing project', command: 'node', args: [entry, '--dir', 'init-audit', 'init', '--bare', '--no-init-package-manager'], expectExitCode: 1, expectOutput: 'already has a package.json file' },
    { name: 'set a project script', command: 'node', args: [entry, 'set-script', 'audit-script', `node -e "console.log('wasm-set-script-ok')"`] },
    { name: 'run the new project script', command: 'node', args: [entry, 'run', 'audit-script'], expectOutput: 'wasm-set-script-ok' },
    { name: 'set a test script', command: 'node', args: [entry, 'set-script', 'test', `node -e "console.log('wasm-test-shortcut-ok')"`] },
    { name: 'run the test shortcut', command: 'node', args: [entry, 'test'], expectOutput: 'wasm-test-shortcut-ok' },
    { name: 'run the t test alias', command: 'node', args: [entry, 't'], expectOutput: 'wasm-test-shortcut-ok' },
    { name: 'run the tst test alias', command: 'node', args: [entry, 'tst'], expectOutput: 'wasm-test-shortcut-ok' },
    { name: 'reject an unsafe script key', command: 'node', args: [entry, 'set-script', 'constructor', 'echo', 'unsafe'], expectExitCode: 1, expectOutput: 'ERR_PNPM_UNSAFE_PROPERTY_PATH_KEY' },
    { name: 'reject set-script without a command', command: 'node', args: [entry, 'set-script', 'audit-script'], expectExitCode: 1, expectOutput: 'ERR_PNPM_SET_SCRIPT_MISSING_ARGS' },
    { name: 'read a package manifest value', command: 'node', args: [entry, 'pkg', 'get', 'name'], expectOutput: 'webcontainer-e2e-app' },
    { name: 'set a package manifest value', command: 'node', args: [entry, 'pkg', 'set', 'description=WebContainer audit'] },
    { name: 'verify package manifest edit', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('./package.json').description, 'WebContainer audit')"] },
    { name: 'delete a package manifest value', command: 'node', args: [entry, 'pkg', 'delete', 'description'] },
    { name: 'reject an unsafe package property path', command: 'node', args: [entry, 'pkg', 'set', 'constructor.polluted=true'], expectExitCode: 1, expectOutput: 'ERR_PNPM_UNSAFE_PROPERTY_PATH_KEY' },
    { name: 'generate bash completion', command: 'node', args: [entry, 'completion', 'bash'], expectOutput: 'complete' },
    { name: 'reject an unsupported completion shell', command: 'node', args: [entry, 'completion', 'unsupported-shell'], expectExitCode: 1, expectOutput: 'ERR_PNPM_UNSUPPORTED_SHELL' },
    { name: 'inspect task concurrency groups', command: 'node', args: [entry, 'tasks', 'status'], expectOutput: 'No concurrency groups are in use' },
    { name: 'reject tasks without a subcommand', command: 'node', args: [entry, 'tasks'], expectExitCode: 1, expectOutput: 'a tasks subcommand is required' },
    { name: 'diagnose the WebContainer offline', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--doctor', entry], expectOutput: 'offline checks' },
    { name: 'run scripts with the recursive command', command: 'node', args: [entry, 'recursive', 'run', 'build'], expectOutput: 'wasm-filter-ok' },
    { name: 'list dependencies in long format with ll', command: 'node', args: [entry, 'll'], expectOutput: '4 packages' },
    { name: 'long list alias la', command: 'node', args: [entry, 'la'], expectOutput: '4 packages' },
    { name: 'create an install-test project', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--install-test-fixture'] },
    { name: 'install dependencies and run tests', command: 'node', args: [entry, '--dir', 'install-test-audit', 'install-test', '--offline'] },
    { name: 'verify install-test ran the test script', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('node:fs').readFileSync('install-test-audit/install-test-ok', 'utf8'), 'yes')"] },
    { name: 'remove install-test marker before alias run', command: 'node', args: ['-e', "require('node:fs').rmSync('install-test-audit/install-test-ok')"] },
    { name: 'remove install-test dependencies before alias run', command: 'rm', args: ['-rf', 'install-test-audit/node_modules'] },
    { name: 'run install-test alias it', command: 'node', args: [entry, '--dir', 'install-test-audit', 'it', '--offline'] },
    { name: 'verify install-test alias ran the test script', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('node:fs').readFileSync('install-test-audit/install-test-ok', 'utf8'), 'yes')"] },
    { name: 'clean install with ci', command: 'node', args: [entry, '--dir', 'install-test-audit', 'ci', '--offline'] },
    { name: 'verify ci install', command: 'node', args: ['-e', "if (!require('node:fs').existsSync('install-test-audit/node_modules')) throw new Error('ci did not create node_modules')"] },
    { name: 'prepare clean-install sentinel', command: 'node', args: ['-e', "require('node:fs').writeFileSync('install-test-audit/node_modules/clean-install-sentinel', 'stale')"] },
    { name: 'run ci alias clean-install', command: 'node', args: [entry, '--dir', 'install-test-audit', 'clean-install', '--offline'] },
    { name: 'verify clean-install removed old files and restored dependencies', command: 'node', args: ['-e', "const fs = require('node:fs'); require('node:assert/strict').equal(fs.existsSync('install-test-audit/node_modules/clean-install-sentinel'), false); require('node:assert/strict').equal(require('./install-test-audit/node_modules/semver').valid('1.2.3'), '1.2.3')"] },
    { name: 'prepare install-clean sentinel', command: 'node', args: ['-e', "require('node:fs').writeFileSync('install-test-audit/node_modules/install-clean-sentinel', 'stale')"] },
    { name: 'run ci alias install-clean', command: 'node', args: [entry, '--dir', 'install-test-audit', 'install-clean', '--offline'] },
    { name: 'verify install-clean removed old files and restored dependencies', command: 'node', args: ['-e', "const fs = require('node:fs'); require('node:assert/strict').equal(fs.existsSync('install-test-audit/node_modules/install-clean-sentinel'), false); require('node:assert/strict').equal(require('./install-test-audit/node_modules/semver').valid('1.2.3'), '1.2.3')"] },
    { name: 'create an isolated clean tree with a nested directory link', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--clean-tree-fixture'] },
    { name: 'clean the isolated tree with a nested directory link', command: 'node', args: [entry, '--dir', '/tmp/pnpm-clean-audit', 'clean'] },
    { name: 'verify isolated clean removed .pnpm', command: 'node', args: ['-e', "if (require('node:fs').existsSync('/tmp/pnpm-clean-audit/node_modules/.pnpm')) throw new Error('clean left .pnpm')"] },
    { name: 'read a package store index', command: 'node', args: [entry, 'cat-index', 'semver@7.6.3'], expectOutput: '"files"' },
    { name: 'report a missing package store index', command: 'node', args: [entry, 'cat-index', 'missing-package@1.0.0'], expectExitCode: 1, expectOutput: 'No corresponding index file found' },
    { name: 'read content by its store hash', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--cat-file', entry], expectOutput: 'cat-file returned the matching content-addressed package file' },
    { name: 'reject an invalid store hash', command: 'node', args: [entry, 'cat-file', 'invalid-hash'], expectExitCode: 1, expectOutput: 'ERR_PNPM_INVALID_HASH' },
    { name: 'list registries in the metadata cache', command: 'node', args: [entry, 'cache', 'list-registries'], expectOutput: 'registry.npmjs.org' },
    { name: 'list cached package metadata', command: 'node', args: [entry, 'cache', 'list', 'semver'], expectOutput: 'semver.jsonl' },
    { name: 'view cached package metadata', command: 'node', args: [entry, 'cache', 'view', 'semver'], expectOutput: '7.6.3' },
    { name: 'reject cache path traversal', command: 'node', args: [entry, 'cache', 'view', '../outside'], expectExitCode: 1, expectOutput: 'path traversal sequences are not allowed' },
    { name: 'prune unused cache entries without deleting them', command: 'node', args: [entry, 'cache', 'prune', '--dry-run'] },
    { name: 'delete cached metadata for one package', command: 'node', args: [entry, 'cache', 'delete', 'semver'] },
    { name: 'verify deleted metadata cache entry', command: 'node', args: [entry, 'cache', 'view', 'semver'], expectOutput: '{}' },
    { name: 'prune unused store packages', command: 'node', args: [entry, 'store', 'prune'] },
    { name: 'verify installed packages survive store pruning', command: 'node', args: ['-e', "require('node:assert/strict').equal(require('./packages/local/node_modules/semver').valid('1.2.3'), '1.2.3')"] },
  ]
}

export function cleanCapabilitySteps (entry) {
  return [
    { name: 'clean workspace dependencies', command: 'node', args: [entry, 'clean'], env: { npm_config_manage_package_manager_versions: 'false' }, expectExitCode: 1, expectOutput: 'Directory not empty' },
    { name: 'inspect entries left by WASI cleanup', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--list-clean-remnants'], expectOutput: 'clean left' },
    { name: 'restore workspace dependencies before purge probe', command: 'node', args: [entry, 'install', '--offline'], env: { npm_config_manage_package_manager_versions: 'false' } },
    { name: 'verify purge probe starts with installed dependencies', command: 'node', args: ['-e', "if (!require('node:fs').existsSync('node_modules/.pnpm')) throw new Error('purge probe has no installed virtual store')"] },
    { name: 'clean alias purge reports the same filesystem error', command: 'node', args: [entry, 'purge'], env: { npm_config_manage_package_manager_versions: 'false' }, expectExitCode: 1, expectOutput: 'Directory not empty' },
    { name: 'remove remaining virtual store through the WebContainer Node API', command: 'node', args: ['-e', "require('node:fs').rmSync('node_modules/.pnpm', { recursive: true, force: true })"] },
    { name: 'verify Node recursive removal cleared the virtual store', command: 'node', args: ['-e', "if (require('node:fs').existsSync('node_modules/.pnpm')) throw new Error('Node recursive removal left .pnpm')"] },
  ]
}

export function networkCapabilitySteps (entry) {
  return [
    { name: 'prepare TLS capability fixtures', command: 'node', args: ['../pnpm/cli-fixture.mjs', '--network-cert-fixtures'] },
    {
      name: 'reject an explicit HTTP proxy without side effects',
      command: 'node',
      args: ['../pnpm/cli-fixture.mjs', '--assert-no-effects', entry, '--http-proxy', 'ping', '--expect', 'Explicit HTTP proxy configuration is unavailable in WebContainers'],
      expectOutput: 'failed without changing project files',
    },
    {
      name: 'reject disabled TLS verification without side effects',
      command: 'node',
      args: ['../pnpm/cli-fixture.mjs', '--assert-no-effects', entry, 'ping', '--config.strict-ssl=false', '--expect', 'Disabled TLS verification is unavailable in WebContainers'],
      expectOutput: 'failed without changing project files',
    },
    {
      name: 'reject pinned local addresses without side effects',
      command: 'node',
      args: ['../pnpm/cli-fixture.mjs', '--assert-no-effects', entry, 'ping', '--config.local-address=127.0.0.1', '--expect', 'Pinned local network addresses is unavailable in WebContainers'],
      expectOutput: 'failed without changing project files',
    },
    {
      name: 'reject custom TLS trust roots without side effects',
      command: 'node',
      args: ['../pnpm/cli-fixture.mjs', '--assert-no-effects', entry, 'ping', '--config.cafile=/home/project/app/test-ca.pem', '--expect', 'Custom TLS trust roots is unavailable in WebContainers'],
      expectOutput: 'failed without changing project files',
    },
    {
      name: 'reject client TLS identities without side effects',
      command: 'node',
      args: ['../pnpm/cli-fixture.mjs', '--client-identity-config'],
    },
    {
      name: 'reject client TLS identities without side effects',
      command: 'node',
      args: ['../pnpm/cli-fixture.mjs', '--assert-no-effects', entry, 'ping', '--expect', 'Client TLS identities are unavailable in WebContainers'],
      expectOutput: 'failed without changing project files',
    },
    { name: 'remove client TLS identity config fixture', command: 'node', args: ['-e', "require('node:fs').unlinkSync('.npmrc')"] },
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
