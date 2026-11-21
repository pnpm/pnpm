import { spawn, spawnSync } from 'node:child_process'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { appendFileSync, lstatSync, mkdirSync, readFileSync, readdirSync, readlinkSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { stripVTControlCharacters } from 'node:util'
import { createServer } from 'node:http'

if (process.argv[2] === '--matching-pin') await installWithMatchingPin(process.argv[3])
else if (process.argv[2] === '--approval') createApprovalFixture()
else if (process.argv[2] === '--frontend') createFrontendFixture()
else if (process.argv[2] === '--assert-no-effects') assertUnsupportedCommandHasNoEffects(process.argv[3])
else if (process.argv[2] === '--registry-workflow') await runRegistryWorkflow(process.argv[3])
else if (process.argv[2] === '--store-status-mutation') assertStoreStatusDetectsMutation(process.argv[3])
else if (process.argv[2] === '--cat-file') assertCatFileContents(process.argv[3])
else if (process.argv[2] === '--find-hash') assertFindHash(process.argv[3])
else if (process.argv[2] === '--network-cert-fixtures') writeNetworkCertificateFixtures()
else if (process.argv[2] === '--client-identity-config') writeClientIdentityConfig()
else if (process.argv[2] === '--install-test-fixture') createInstallTestFixture()
else if (process.argv[2] === '--link-fixture') createLinkFixture()
else if (process.argv[2] === '--doctor') inspectDoctor(process.argv[3])
else if (process.argv[2] === '--command-routing') assertCommandRouting(process.argv[3], process.argv[4])
else if (process.argv[2] === '--list-clean-remnants') listCleanRemnants()
else if (process.argv[2] === '--clean-tree-fixture') createCleanTreeFixture()
else createWorkspaceFixture()

async function runRegistryWorkflow (entry) {
  const requests = []
  const redirectRequests = []
  const stars = new Set()
  const rejectedStages = new Set()
  const redirectServer = createServer((request, response) => {
    redirectRequests.push({ method: request.method, url: request.url, authorization: request.headers.authorization })
    response.setHeader('content-type', 'application/json')
    response.end(JSON.stringify({ ping: 'pong' }))
  })
  await new Promise(resolve => redirectServer.listen(0, '127.0.0.1', resolve))
  const redirectTarget = `http://127.0.0.1:${redirectServer.address().port}`
  const stagedId = '1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f'
  const stagedItem = {
    id: stagedId,
    packageName: 'webcontainer-publish-probe',
    version: '1.0.0',
    tag: 'latest',
    createdAt: '2026-03-16T09:00:00.000Z',
    actor: 'webcontainer-fixture',
    actorType: 'user',
  }
  let publishDocument
  const server = createServer(async (request, response) => {
    const chunks = []
    for await (const chunk of request) chunks.push(chunk)
    const body = Buffer.concat(chunks).toString()
    const pathname = request.url.split('?')[0]
    requests.push({ method: request.method, url: request.url, authorization: request.headers.authorization, body })
    if (request.method === 'GET' && pathname === '/redirect/-/ping') {
      response.writeHead(302, { location: `${redirectTarget}/-/ping` }).end()
    } else if (request.method === 'GET' && pathname === '/-/stage') {
      const items = rejectedStages.has(stagedId) ? [] : [stagedItem]
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({ items, total: items.length }))
    } else if (request.method === 'GET' && pathname === `/-/stage/${stagedId}`) {
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify(stagedItem))
    } else if (request.method === 'DELETE' && pathname === `/-/stage/${stagedId}`) {
      rejectedStages.add(stagedId)
      response.writeHead(204).end()
    } else if (request.method === 'GET' && pathname === '/semver') {
      const integrity = `sha512-${Buffer.alloc(64).toString('base64')}`
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({
        name: 'semver',
        'dist-tags': { latest: '7.7.2' },
        versions: Object.fromEntries(['7.6.3', '7.7.2'].map(version => [version, {
          name: 'semver',
          version,
          description: 'WebContainer command audit fixture',
          license: 'ISC',
          dist: { integrity, tarball: `${registry}semver/-/semver-${version}.tgz` },
        }])),
      }))
    } else if (request.method === 'POST' && pathname === '/-/npm/v1/security/advisories/bulk') {
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({ semver: [] }))
    } else if (request.method === 'POST' && request.url === '/-/v1/login') {
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({ loginUrl: `http://127.0.0.1:${server.address().port}/login`, doneUrl: `http://127.0.0.1:${server.address().port}/done` }))
    } else if (request.url.startsWith('/done')) {
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({ token: 'fixture-token' }))
    } else if (request.method === 'GET' && request.url === '/-/whoami') {
      if (request.headers.authorization === 'Bearer fixture-token') {
        response.setHeader('content-type', 'application/json')
        response.end(JSON.stringify({ username: 'webcontainer-fixture' }))
      } else {
        response.writeHead(401).end('Unauthorized')
      }
    } else if (request.method === 'GET' && request.url === '/-/npm/v1/user') {
      if (request.headers.authorization !== 'Bearer fixture-token') {
        response.writeHead(401).end('Unauthorized')
      } else {
        response.setHeader('content-type', 'application/json')
        response.end(JSON.stringify({ name: 'webcontainer-fixture', email: 'fixture@example.test' }))
      }
    } else if (request.method === 'GET' && request.url === '/-/npm/v1/tokens') {
      if (request.headers.authorization !== 'Bearer fixture-token') {
        response.writeHead(401).end('Unauthorized')
      } else {
        response.setHeader('content-type', 'application/json')
        response.end(JSON.stringify([{ key: 'fixture-key', token: 'npm_fixturetoken', readonly: true }]))
      }
    } else if (request.method === 'GET' && pathname === '/-/ping') {
      response.writeHead(request.url.startsWith('/missing/') ? 404 : 200, { 'content-type': 'application/json' })
      response.end(JSON.stringify({ ping: 'pong' }))
    } else if (request.method === 'GET' && pathname === '/-/v1/search') {
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({ objects: [{ package: { name: 'webcontainer-publish-probe', version: '1.0.0', description: 'WebContainer registry fixture' } }] }))
    } else if (request.method === 'PUT' && pathname === '/-/user/v1/star') {
      if (request.headers.authorization !== 'Bearer fixture-token') response.writeHead(401).end('Unauthorized')
      else {
        stars.add(JSON.parse(body).package)
        response.writeHead(201).end('{}')
      }
    } else if (request.method === 'DELETE' && pathname === '/-/user/v1/star') {
      if (request.headers.authorization !== 'Bearer fixture-token') response.writeHead(401).end('Unauthorized')
      else {
        stars.delete(JSON.parse(body).package)
        response.writeHead(200).end('{}')
      }
    } else if (request.method === 'GET' && pathname === '/-/user/v1/star') {
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify([...stars]))
    } else if (request.method === 'PUT' && pathname === '/webcontainer-publish-probe' && !request.url.includes('?write=true')) {
      if (request.headers.authorization !== 'Bearer fixture-token') {
        response.writeHead(401).end('Unauthorized')
      } else {
        publishDocument = JSON.parse(body)
        response.writeHead(201, { 'content-type': 'application/json' }).end(JSON.stringify({ ok: true }))
      }
    } else if (request.method === 'GET' && pathname === '/webcontainer-publish-probe') {
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({ ...(publishDocument ?? { name: 'webcontainer-publish-probe', versions: { '1.0.0': { name: 'webcontainer-publish-probe', version: '1.0.0', dist: { tarball: `${registry}webcontainer-publish-probe/-/webcontainer-publish-probe-1.0.0.tgz` } } }, 'dist-tags': { latest: '1.0.0' } }), _rev: '3-abc' }))
    } else if (pathname === '/-/package/webcontainer-publish-probe/dist-tags') {
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({ latest: '1.0.0', beta: '1.0.0' }))
    } else if (request.method === 'PUT' && pathname.startsWith('/-/package/webcontainer-publish-probe/dist-tags/')) {
      response.writeHead(201).end()
    } else if (request.method === 'DELETE' && pathname.startsWith('/-/package/webcontainer-publish-probe/dist-tags/')) {
      response.writeHead(200).end()
    } else if (request.method === 'PUT' && pathname === '/-/package/webcontainer-publish-probe/owners') {
      response.writeHead(200).end()
    } else if (request.method === 'POST' && pathname === '/-/package/@fixture%2fwebcontainer-publish-probe/access') {
      response.writeHead(200).end()
    } else if (request.method === 'PUT' && pathname === '/-/org/myscope/team') {
      response.writeHead(201).end()
    } else if (request.method === 'PUT' && pathname === '/webcontainer-publish-probe') {
      publishDocument = JSON.parse(body)
      response.writeHead(200, { 'content-type': 'application/json' }).end('{}')
    } else if (request.method === 'DELETE' && pathname.startsWith('/webcontainer-publish-probe/-rev/')) {
      response.writeHead(200).end()
    } else if (request.method === 'DELETE' && request.url === '/-/user/token/fixture-token') {
      if (request.headers.authorization !== 'Bearer fixture-token') response.writeHead(401).end('Unauthorized')
      else response.writeHead(204).end()
    } else {
      response.writeHead(404).end('Not found')
    }
  })
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve))
  const registry = `http://127.0.0.1:${server.address().port}/`
  const project = path.resolve('.registry-probe')
  const home = path.resolve('.registry-probe-home')
  mkdirSync(project, { recursive: true })
  mkdirSync(home, { recursive: true })
  writeFileSync(path.join(project, 'package.json'), JSON.stringify({
    name: 'webcontainer-publish-probe',
    version: '1.0.0',
    bugs: { url: 'https://example.test/webcontainer-publish-probe/issues' },
    homepage: 'https://example.test/webcontainer-publish-probe',
    repository: 'https://example.test/webcontainer-publish-probe.git',
  }))
  const env = {
    ...process.env,
    HOME: home,
    PNPM_HOME: path.join(home, 'pnpm'),
    XDG_CACHE_HOME: path.join(home, 'cache'),
    XDG_CONFIG_HOME: path.join(home, 'config'),
    XDG_DATA_HOME: path.join(home, 'data'),
    npm_config_userconfig: '/dev/null',
  }
  try {
    const login = await runInteractivePackageManager(entry, ['login', '--registry', registry], project, env)
    assert.equal(login.exitCode, 0, `login exited with ${login.exitCode}`)

    const view = await runPackageManager(entry, ['view', 'webcontainer-publish-probe', 'version', '--json', '--registry', registry], project, env)
    assert.equal(view.exitCode, 0, view.output)
    assert.equal(JSON.parse(view.output), '1.0.0')
    for (const command of ['info', 'show', 'v']) {
      const alias = await runPackageManager(entry, [command, 'webcontainer-publish-probe', 'version', '--json', '--registry', registry], project, env)
      assert.equal(alias.exitCode, 0, `${command}: ${alias.output}`)
      assert.equal(JSON.parse(alias.output), '1.0.0')
    }

    const missingView = await runPackageManager(entry, ['view', 'missing-webcontainer-probe', '--registry', registry], project, env)
    assert.equal(missingView.exitCode, 1)
    assert.match(missingView.output, /ERR_PNPM_FETCH_404/)

    const search = await runPackageManager(entry, ['search', 'webcontainer', '--json', '--registry', registry], project, env)
    assert.equal(search.exitCode, 0, search.output)
    assert.equal(JSON.parse(search.output)[0].name, 'webcontainer-publish-probe')
    for (const command of ['find', 's', 'se']) {
      const alias = await runPackageManager(entry, [command, 'webcontainer', '--json', '--registry', registry], project, env)
      assert.equal(alias.exitCode, 0, `${command}: ${alias.output}`)
      assert.equal(JSON.parse(alias.output)[0].name, 'webcontainer-publish-probe')
    }

    const missingSearch = await runPackageManager(entry, ['search', '--registry', registry], project, env)
    assert.equal(missingSearch.exitCode, 1)
    assert.match(missingSearch.output, /ERR_PNPM_MISSING_SEARCH_QUERY/)

    const ping = await runPackageManager(entry, ['ping', '--registry', registry], project, env)
    assert.equal(ping.exitCode, 0, ping.output)
    assert.match(ping.output, /PONG.*ping.*pong/s)

    writeFileSync(path.join(project, '.npmrc'), `//127.0.0.1:${server.address().port}/:_authToken=fixture-token\n`)
    const redirectPing = await runPackageManager(entry, ['ping', '--registry', `${registry}redirect/`], project, env)
    rmSync(path.join(project, '.npmrc'), { force: true })
    assert.equal(redirectPing.exitCode, 0, redirectPing.output)
    assert.ok(requests.some(request => request.url.startsWith('/redirect/-/ping') && request.authorization === 'Bearer fixture-token'), JSON.stringify(requests.map(({ url, authorization }) => ({ url, authorization }))))
    assert.ok(redirectRequests.some(request => request.url === '/-/ping' && request.authorization == null), `cross-origin redirect forwarded registry credentials: ${JSON.stringify(redirectRequests)}`)

    const failedPing = await runPackageManager(entry, ['ping', '--registry', `${registry}missing/`], project, env)
    assert.equal(failedPing.exitCode, 1)
    assert.match(failedPing.output, /ERR_PNPM_PING_ERROR/)

    const star = await runPackageManager(entry, ['star', 'webcontainer-publish-probe', '--registry', registry], project, env)
    assert.equal(star.exitCode, 0, star.output)
    const starred = await runPackageManager(entry, ['stars', '--registry', registry], project, env)
    assert.equal(starred.exitCode, 0, starred.output)
    assert.match(starred.output, /webcontainer-publish-probe/)
    const unstar = await runPackageManager(entry, ['unstar', 'webcontainer-publish-probe', '--registry', registry], project, env)
    assert.equal(unstar.exitCode, 0, unstar.output)
    const stars = await runPackageManager(entry, ['stars', '--registry', registry], project, env)
    assert.equal(stars.exitCode, 0, stars.output)
    assert.doesNotMatch(stars.output, /webcontainer-publish-probe/)

    const whoami = await runPackageManager(entry, ['whoami', '--registry', registry], project, env)
    assert.equal(whoami.exitCode, 0, whoami.output)
    assert.match(whoami.output, /webcontainer-fixture/)

    const profile = await runPackageManager(entry, ['profile', 'get', 'email', '--json', '--registry', registry], project, env)
    assert.equal(profile.exitCode, 0, profile.output)
    assert.deepEqual(JSON.parse(profile.output), { email: 'fixture@example.test' })

    const tokens = await runPackageManager(entry, ['token', 'list', '--json', '--registry', registry], project, env)
    assert.equal(tokens.exitCode, 0, tokens.output)
    assert.deepEqual(JSON.parse(tokens.output).map(({ key, readonly }) => ({ key, readonly })), [{ key: 'fixture-key', readonly: true }])

    const bugs = await runPackageManager(entry, ['bugs'], project, env)
    assert.equal(bugs.exitCode, 0, bugs.output)
    assert.match(bugs.output, /https:\/\/example\.test\/webcontainer-publish-probe\/issues/)
    const issues = await runPackageManager(entry, ['issues'], project, env)
    assert.equal(issues.exitCode, 0, issues.output)
    assert.match(issues.output, /https:\/\/example\.test\/webcontainer-publish-probe\/issues/)

    const repo = await runPackageManager(entry, ['repo'], project, env)
    assert.equal(repo.exitCode, 0, repo.output)
    assert.match(repo.output, /https:\/\/example\.test\/webcontainer-publish-probe/)
    assert.match(repo.output, /Could not open browser: WebContainers cannot open a browser window/)

    const emptyProject = path.resolve('.empty-project-probe')
    mkdirSync(emptyProject, { recursive: true })
    writeFileSync(path.join(emptyProject, 'package.json'), JSON.stringify({ name: 'empty-project-probe', version: '1.0.0', dependencies: { semver: '7.6.3' } }))
    writeFileSync(path.join(emptyProject, 'pnpm-workspace.yaml'), 'packages: []\n')
    const missingBugs = await runPackageManager(entry, ['bugs'], emptyProject, env)
    assert.equal(missingBugs.exitCode, 1)
    assert.match(missingBugs.output, /ERR_PNPM_NO_BUGS_URL/)
    const missingRepo = await runPackageManager(entry, ['repo'], emptyProject, env)
    assert.equal(missingRepo.exitCode, 1)
    assert.match(missingRepo.output, /ERR_PNPM_NO_REPO_URL/)

    const outdatedProject = path.resolve('.outdated-probe')
    createLockedDependencyFixture(outdatedProject)
    const outdated = await runPackageManager(entry, ['outdated', '--format', 'json', '--registry', registry], outdatedProject, env)
    assert.equal(outdated.exitCode, 1, outdated.output)
    const outdatedReport = parseJsonOutput(outdated.output, 'outdated --format json')
    assert.equal(outdatedReport.semver.current, '7.6.3')
    assert.equal(outdatedReport.semver.latest, '7.7.2')

    const emptyProjectBeforeOutdated = snapshot(emptyProject)
    const requestsBeforeOutdatedWithoutLockfile = requests.length
    const outdatedWithoutLockfile = await runPackageManager(entry, ['outdated', '--registry', registry], emptyProject, env)
    assert.equal(outdatedWithoutLockfile.exitCode, 1, outdatedWithoutLockfile.output)
    assert.match(stripVTControlCharacters(outdatedWithoutLockfile.output), /semver.*7\.6\.3.*7\.7\.2/s)
    assert.deepEqual(snapshot(emptyProject), emptyProjectBeforeOutdated, 'outdated without a lockfile changed project files')
    assert.ok(requests.length > requestsBeforeOutdatedWithoutLockfile, 'outdated without a lockfile did not query the registry')

    const stageList = await runPackageManager(entry, ['stage', 'list', '--json', '--registry', registry], project, env)
    assert.equal(stageList.exitCode, 0, stageList.output)
    assert.match(stageList.output, /^\s*\[/, stageList.output)
    assert.equal(parseJsonOutput(stageList.output, 'stage list --json')[0].packageName, stagedItem.packageName)
    const stageView = await runPackageManager(entry, ['stage', 'view', stagedId, '--json', '--registry', registry], project, env)
    assert.equal(stageView.exitCode, 0, stageView.output)
    assert.match(stageView.output, /^\s*\{/, stageView.output)
    assert.equal(parseJsonOutput(stageView.output, 'stage view --json').id, stagedId)
    const stageInvalidId = await runPackageManager(entry, ['stage', 'view', 'not-a-uuid', '--registry', registry], project, env)
    assert.equal(stageInvalidId.exitCode, 1)
    assert.match(stageInvalidId.output, /ERR_PNPM_INVALID_STAGE_ID/)
    const stageRequestsBeforeReject = requests.length
    const stageReject = await runPackageManager(entry, ['stage', 'reject', stagedId, '--registry', registry], project, env)
    assert.equal(stageReject.exitCode, 0, stageReject.output)
    assert.ok(requests.slice(stageRequestsBeforeReject).some(request => request.method === 'DELETE' && request.url === `/-/stage/${stagedId}`))
    const stageListAfterReject = await runPackageManager(entry, ['stage', 'list', '--json', '--registry', registry], project, env)
    assert.equal(stageListAfterReject.exitCode, 0, stageListAfterReject.output)
    assert.deepEqual(parseJsonOutput(stageListAfterReject.output, 'stage list after reject --json'), [])

    const stagePublishDryRun = await runPackageManager(entry, ['stage', 'publish', '--dry-run', '--no-git-checks', '--registry', registry], project, env)
    assert.equal(stagePublishDryRun.exitCode, 0, stagePublishDryRun.output)
    assert.match(stagePublishDryRun.output, /would stage/)

    const audit = await runPackageManager(entry, ['audit', '--json', '--registry', registry], outdatedProject, env)
    assert.equal(audit.exitCode, 0, audit.output)
    assert.deepEqual(parseJsonOutput(audit.output, 'audit --json').metadata.vulnerabilities, { info: 0, low: 0, moderate: 0, high: 0, critical: 0 })
    assert.ok(requests.some(request => request.method === 'POST' && request.url === '/-/npm/v1/security/advisories/bulk' && JSON.parse(request.body).semver?.includes('7.6.3')))

    const requestsBeforeUnknownAudit = requests.length
    const auditUnknownSubcommand = await runPackageManager(entry, ['audit', 'unknown', '--registry', registry], outdatedProject, env)
    assert.equal(auditUnknownSubcommand.exitCode, 1)
    assert.match(auditUnknownSubcommand.output, /ERR_PNPM_AUDIT_UNKNOWN_SUBCOMMAND/)
    assert.equal(requests.length, requestsBeforeUnknownAudit, 'unknown audit subcommand reached the registry')

    const publish = await runPackageManager(entry, ['publish', '--registry', registry, '--no-git-checks', '--ignore-scripts', '--publish-wait-timeout', '0'], project, env)
    assert.equal(publish.exitCode, 0, publish.output)
    assert.equal(publishDocument?.name, 'webcontainer-publish-probe')
    assert.ok(publishDocument?.versions?.['1.0.0'])
    assert.ok(publishDocument?._attachments?.['webcontainer-publish-probe-1.0.0.tgz']?.data)

    for (const command of ['docs', 'home']) {
      const result = await runPackageManager(entry, [command, 'webcontainer-publish-probe', '--registry', registry], project, env)
      assert.equal(result.exitCode, 0, result.output)
      assert.match(result.output, /Could not open browser: WebContainers cannot open a browser window/)
      assert.match(result.output, /https:\/\/example\.test\/webcontainer-publish-probe/)
    }

    for (const [args, method, url] of [
      [['dist-tag', 'add', 'webcontainer-publish-probe@1.0.0', 'beta', '--registry', registry], 'PUT', '/-/package/webcontainer-publish-probe/dist-tags/beta'],
      [['dist-tags', 'add', 'webcontainer-publish-probe@1.0.0', 'next', '--registry', registry], 'PUT', '/-/package/webcontainer-publish-probe/dist-tags/next'],
      [['dist-tag', 'rm', 'webcontainer-publish-probe', 'beta', '--registry', registry], 'DELETE', '/-/package/webcontainer-publish-probe/dist-tags/beta'],
      [['deprecate', 'webcontainer-publish-probe@1.0.0', 'fixture retirement', '--registry', registry], 'PUT', '/webcontainer-publish-probe'],
      [['undeprecate', 'webcontainer-publish-probe@1.0.0', '--registry', registry], 'PUT', '/webcontainer-publish-probe'],
      [['owner', 'add', 'webcontainer-publish-probe', 'fixture-owner', '--registry', registry], 'PUT', '/-/package/webcontainer-publish-probe/owners'],
      [['owners', 'add', 'webcontainer-publish-probe', 'fixture-owner', '--registry', registry], 'PUT', '/-/package/webcontainer-publish-probe/owners'],
      [['access', 'set', 'status=public', '@fixture/webcontainer-publish-probe', '--registry', registry], 'POST', '/-/package/@fixture%2fwebcontainer-publish-probe/access'],
      [['team', 'create', '@myscope:developers', '--registry', registry], 'PUT', '/-/org/myscope/team'],
    ]) {
      const requestCount = requests.length
      const result = await runPackageManager(entry, args, project, env)
      assert.equal(result.exitCode, 0, `${args.join(' ')}: ${result.output}`)
      assert.ok(requests.slice(requestCount).some(request => request.method === method && new URL(request.url, registry).pathname === url), `${args.join(' ')} did not call ${method} ${url}`)
      if (args[0] === 'deprecate') assert.equal(publishDocument?.versions?.['1.0.0']?.deprecated, 'fixture retirement')
      if (args[0] === 'undeprecate') assert.equal(publishDocument?.versions?.['1.0.0']?.deprecated, '')
    }

    const unpublish = await runPackageManager(entry, ['unpublish', 'webcontainer-publish-probe', '--force', '--registry', registry], project, env)
    assert.equal(unpublish.exitCode, 0, unpublish.output)
    assert.ok(requests.some(request => request.method === 'DELETE' && request.url.startsWith('/webcontainer-publish-probe/-rev/')))

    const logout = await runPackageManager(entry, ['logout', '--registry', registry], project, env)
    assert.equal(logout.exitCode, 0, logout.output)
    assert.ok(requests.some(request => request.method === 'DELETE' && request.url === '/-/user/token/fixture-token' && request.authorization === 'Bearer fixture-token'))

    const unauthorized = await runPackageManager(entry, ['whoami', '--registry', registry], project, env)
    assert.equal(unauthorized.exitCode, 1)
    assert.match(unauthorized.output, /You must be logged in to use whoami/)
    const unauthorizedProfile = await runPackageManager(entry, ['profile', 'get', '--registry', registry], project, env)
    assert.equal(unauthorizedProfile.exitCode, 1)
    assert.match(unauthorizedProfile.output, /You must be logged in/)
    const unauthorizedTokens = await runPackageManager(entry, ['token', 'list', '--registry', registry], project, env)
    assert.equal(unauthorizedTokens.exitCode, 1)
    assert.match(unauthorizedTokens.output, /You must be logged in to view or change your tokens/)
    const unauthorizedStar = await runPackageManager(entry, ['star', 'webcontainer-publish-probe', '--registry', registry], project, env)
    assert.equal(unauthorizedStar.exitCode, 1)
    assert.match(unauthorizedStar.output, /ERR_PNPM_STAR_UNAUTHORIZED/)
    const unauthorizedStars = await runPackageManager(entry, ['stars', '--registry', registry], project, env)
    assert.equal(unauthorizedStars.exitCode, 1)
    assert.match(unauthorizedStars.output, /ERR_PNPM_STARS_UNAUTHORIZED/)
    const adduser = await runInteractivePackageManager(entry, ['adduser', '--registry', registry], project, env)
    assert.equal(adduser.exitCode, 0, `adduser exited with ${adduser.exitCode}`)
    const aliasWhoami = await runPackageManager(entry, ['whoami', '--registry', registry], project, env)
    assert.equal(aliasWhoami.exitCode, 0, aliasWhoami.output)
    assert.match(aliasWhoami.output, /webcontainer-fixture/)
    const aliasLogout = await runPackageManager(entry, ['logout', '--registry', registry], project, env)
    assert.equal(aliasLogout.exitCode, 0, aliasLogout.output)
    assert.ok(requests.some(request => request.method === 'PUT' && request.url === '/webcontainer-publish-probe'))
    console.log('Mock registry view, search, ping, audit, outdated, stars, staged publish, login, whoami, profile, token list, docs, publish, unpublish, access, owners, teams, tags, deprecation and logout passed')
  } finally {
    await new Promise(resolve => server.close(resolve))
    await new Promise(resolve => redirectServer.close(resolve))
    rmSync(project, { recursive: true, force: true })
    rmSync(home, { recursive: true, force: true })
  }
}

function runPackageManager (entry, args, cwd, env) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [path.resolve(entry), ...args], { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] })
    let output = ''
    child.stdout.setEncoding('utf8').on('data', chunk => { output += chunk })
    child.stderr.setEncoding('utf8').on('data', chunk => { output += chunk })
    child.once('error', reject)
    child.once('close', (exitCode, signal) => resolve({ exitCode: exitCode ?? 1, signal, output }))
  })
}

function runInteractivePackageManager (entry, args, cwd, env) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [path.resolve(entry), ...args], { cwd, env, stdio: 'inherit' })
    child.once('error', reject)
    child.once('close', (exitCode, signal) => resolve({ exitCode: exitCode ?? 1, signal }))
  })
}

function assertStoreStatusDetectsMutation (entry) {
  const packageManifest = path.resolve('node_modules/.pnpm/chalk@4.1.2/node_modules/chalk/package.json')
  const original = readFileSync(packageManifest)
  try {
    writeFileSync(packageManifest, Buffer.concat([original, Buffer.from('\n')]))
    const result = spawnSync(process.execPath, [entry, 'store', 'status'], { encoding: 'utf8' })
    const output = `${result.stdout ?? ''}${result.stderr ?? ''}`
    if (result.status !== 1 || !output.includes('ERR_PNPM_MODIFIED_DEPENDENCY')) {
      throw new Error(`store status did not report a modified package: ${output}`)
    }
    console.log('store status detected a modified installed package')
  } finally {
    writeFileSync(packageManifest, original)
  }
}

function assertCatFileContents (entry) {
  const packageJson = readFileSync('packages/local/node_modules/semver/package.json')
  const hash = createHash('sha512').update(packageJson).digest('base64')
  const result = spawnSync(process.execPath, [path.resolve(entry), 'cat-file', `sha512-${hash}`], { encoding: 'utf8' })
  assert.equal(result.status, 0, result.stderr)
  assert.equal(result.stdout, packageJson.toString())
  console.log('cat-file returned the matching content-addressed package file')
}

function assertFindHash (entry) {
  const packageJson = readFileSync('packages/local/node_modules/semver/package.json')
  const hash = createHash('sha512').update(packageJson).digest('base64')
  const result = spawnSync(process.execPath, [path.resolve(entry), 'find-hash', `sha512-${hash}`], { encoding: 'utf8' })
  assert.equal(result.status, 0, result.stderr || result.stdout)
  assert.match(result.stdout, /semver@7\.6\.3/)
  console.log('find-hash located the package with the matching content hash')
}

function writeNetworkCertificateFixtures () {
  writeFileSync('test-ca.pem', readFileSync('../pnpm/network-ca.pem'))
  writeFileSync('test-client-key.pem', readFileSync('../pnpm/network-client-key.pem'))
}

function writeClientIdentityConfig () {
  writeFileSync('.npmrc', `//registry.npmjs.org/:certfile=${path.resolve('test-ca.pem')}\n//registry.npmjs.org/:keyfile=${path.resolve('test-client-key.pem')}\n`)
}

function assertUnsupportedCommandHasNoEffects (entry) {
  const arguments_ = process.argv.slice(4)
  const expectedIndex = arguments_.indexOf('--expect')
  if (expectedIndex === -1 || expectedIndex === arguments_.length - 1) throw new Error('Expected --expect and a diagnostic')
  const commandArguments = arguments_.slice(0, expectedIndex)
  const useHttpProxy = commandArguments[0] === '--http-proxy'
  if (useHttpProxy) commandArguments.shift()
  const expected = arguments_.slice(expectedIndex + 1).join(' ')
  const cargoProject = commandArguments[0] === '--cargo-add'
  const home = path.resolve('.unsupported-command-home')
  mkdirSync(home, { recursive: true })
  try {
    if (cargoProject) {
      writeFileSync('Cargo.toml', '[package]\nname = "webcontainer-fixture"\nversion = "0.1.0"\nedition = "2024"\n')
      writeFileSync('pnpm-workspace.yaml', 'cargo:\n  enabled: true\n')
      commandArguments.splice(0, 1, 'add', 'crate:fixture@1', '--offline')
    }
    const env = {
      ...process.env,
      HOME: home,
      PNPM_HOME: path.join(home, 'pnpm'),
      XDG_CACHE_HOME: path.join(home, 'cache'),
      XDG_CONFIG_HOME: path.join(home, 'config'),
      XDG_DATA_HOME: path.join(home, 'data'),
      npm_config_userconfig: '/dev/null',
      ...(useHttpProxy ? { HTTPS_PROXY: 'http://127.0.0.1:9999' } : {}),
    }
    const config = spawnSync(process.execPath, [entry, 'config', 'get', 'registry'], {
      cwd: process.cwd(), encoding: 'utf8', env,
    })
    if (config.status !== 0) throw new Error(`failed to initialize isolated config: ${config.stderr}`)
    mkdirSync(env.PNPM_HOME, { recursive: true })
    const before = snapshot(process.cwd())
    const pnpmHomeBefore = snapshot(env.PNPM_HOME)
    const child = spawnSync(process.execPath, [entry, ...commandArguments], {
      cwd: process.cwd(), encoding: 'utf8', env,
    })
    const output = `${child.stdout ?? ''}${child.stderr ?? ''}`
    if (child.status !== 1 || !output.includes(expected)) {
      throw new Error(`${commandArguments.join(' ')} exited with ${child.status}: ${output}`)
    }
    if (JSON.stringify(snapshot(process.cwd())) !== JSON.stringify(before)) {
      throw new Error(`${commandArguments.join(' ')} changed project files before failing`)
    }
    const pnpmHomeAfter = snapshot(env.PNPM_HOME)
    if (JSON.stringify(pnpmHomeAfter) !== JSON.stringify(pnpmHomeBefore)) {
      throw new Error(`${commandArguments.join(' ')} changed PNPM_HOME before failing`)
    }
    console.log(`${commandArguments.join(' ')} failed without changing project files or PNPM_HOME`)
  } finally {
    rmSync(home, { recursive: true, force: true })
    if (cargoProject) {
      rmSync('Cargo.toml', { force: true })
      rmSync('pnpm-workspace.yaml', { force: true })
    }
  }
}

function snapshot (directory) {
  const mode = lstatSync(directory).mode & 0o7777
  const entries = []
  for (const entry of readdirSync(directory, { withFileTypes: true }).sort((left, right) => left.name.localeCompare(right.name))) {
    if (directory === process.cwd() && entry.name === '.unsupported-command-home') continue
    const filename = path.join(directory, entry.name)
    const mode = lstatSync(filename).mode & 0o7777
    if (entry.isDirectory()) entries.push([entry.name, 'directory', mode, snapshot(filename)])
    else if (entry.isSymbolicLink()) entries.push([entry.name, 'symlink', mode, readlinkSync(filename)])
    else entries.push([entry.name, 'file', mode, readFileSync(filename).toString('base64')])
  }
  return [mode, entries]
}

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
    const child = spawn(process.execPath, [entry, 'install', '--trust-lockfile'], {
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
    'wasm-external': 'node -e "console.log(\'wasm-external-script-ok\')"',
    start: 'node -e "console.log(\'wasm-start-script-ok\')"',
    stop: 'node -e "console.log(\'wasm-stop-script-ok\')"',
    restart: 'node -e "console.log(\'wasm-restart-script-ok\')"',
  }
  writeFileSync('package.json', JSON.stringify(manifest))
  writeFileSync('pnpm-workspace.yaml', "packages:\n  - packages/*\npipelines:\n  default: [build]\ntasks:\n  build:\n    dependsOn: []\n    inputs: []\n    outputs: []\n")
  writeFileSync('.pnpmfile.cjs', `module.exports = {
  hooks: {
    readPackage (pkg) {
      require('node:fs').writeFileSync('pnpmfile-hook-ok', pkg.name)
      return pkg
    },
  },
}
`)
  mkdirSync('packages/local', { recursive: true })
  writeFileSync('packages/local/package.json', JSON.stringify({
    name: '@fixture/local', version: '1.0.0', main: 'index.cjs',
    bin: { 'fixture-bin': 'bin.cjs' },
    scripts: {
      build: 'node -e "console.log(\'wasm-filter-ok\')"',
      rebuild: 'node -e "console.log(\'wasm-rebuild-script-override\')"',
    },
    dependencies: { semver: '7.6.3' },
    peerDependencies: { chalk: '^4.0.0' },
  }))
  writeFileSync('packages/local/index.cjs', "module.exports = require('semver').valid('1.2.3')\n")
  writeFileSync('packages/local/bin.cjs', `#!/usr/bin/env node
require('node:assert/strict').deepEqual(process.argv.slice(2), ['a b', '$(false)', "'quoted'"])
console.log('wasm-bin-ok')
`)
}

function createInstallTestFixture () {
  mkdirSync('install-test-audit', { recursive: true })
  writeFileSync('install-test-audit/package.json', JSON.stringify({
    name: 'install-test-audit',
    dependencies: { semver: '7.6.3' },
    scripts: { test: `node -e "require('semver').valid('1.2.3'); require('node:fs').writeFileSync('install-test-ok', 'yes')"` },
  }))
}

function createLinkFixture () {
  mkdirSync('.link-audit', { recursive: true })
  writeFileSync('.link-audit/package.json', JSON.stringify({ name: 'link-audit', version: '1.0.0' }))
}

function createLockedDependencyFixture (directory) {
  mkdirSync(directory, { recursive: true })
  writeFileSync(path.join(directory, 'package.json'), JSON.stringify({
    name: 'webcontainer-command-audit',
    version: '1.0.0',
    dependencies: { semver: '7.6.3' },
  }))
  writeFileSync(path.join(directory, 'pnpm-lock.yaml'), `lockfileVersion: '9.0'
settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false
importers:
  .:
    dependencies:
      semver:
        specifier: 7.6.3
        version: 7.6.3
packages:
  semver@7.6.3:
    resolution:
      integrity: sha512-${Buffer.alloc(64).toString('base64')}
snapshots:
  semver@7.6.3: {}
`)
}

function inspectDoctor (entry) {
  const result = spawnSync(process.execPath, [entry, 'doctor', '--offline', '--json'], { encoding: 'utf8' })
  const report = parseJsonOutput(result.stdout, `doctor --offline --json (exit code ${result.status}, signal ${result.signal}, stderr: ${result.stderr})`)
  assert.ok(Array.isArray(report.checks), 'doctor did not return a check list')
  assert.ok(report.checks.length > 0, 'doctor returned no checks')
  const failures = report.checks.filter(check => check.status === 'fail').map(check => `${check.title}: ${check.detail}`)
  console.log(`doctor completed ${report.checks.length} offline checks with exit code ${result.status}; failed checks: ${failures.join('; ') || 'none'}`)
  assert.equal(result.status, 0, `${result.stderr}\n${failures.join('\n')}`)
}

function assertCommandRouting (entry, inventoryPath) {
  const rows = readFileSync(inventoryPath, 'utf8').trim().split('\n').slice(1)
  const projectBefore = snapshot('.')
  let tested = 0
  for (const row of rows) {
    const [path] = row.split('\t')
    if (path.startsWith('mode:')) continue
    const commandPath = path.startsWith('alias:') ? aliasCommandPath(path) : path
    const args = commandPath.split(' ')
    const help = spawnSync(process.execPath, [entry, ...args, '--help'], { encoding: 'utf8' })
    if (['test', 'start', 'stop'].includes(path) || path === 'alias:test=t' || path === 'alias:test=tst') {
      assert.equal(help.status, 1, `${path} should dispatch to the project script: ${help.stderr || help.stdout}`)
      assert.match(`${help.stderr}${help.stdout}`, /ERR_PNPM_NO_SCRIPT/)
      tested++
      continue
    }
    assert.equal(help.status, 0, `${path} help failed: ${help.stderr || help.stdout}`)
    assert.match(help.stdout, /Usage:/, `${path} did not print command usage`)

    tested++
    if (tested % 25 === 0) console.log(`Routed ${tested} commands and aliases`)
  }
  const invalid = spawnSync(process.execPath, [entry, '--__pnpm_wasm_audit_unknown_option'], { encoding: 'utf8' })
  assert.equal(invalid.status, 2, `pnpm accepted an unknown global option: ${invalid.stderr || invalid.stdout}`)
  assert.deepEqual(snapshot('.'), projectBefore, 'command routing checks changed project files')
  console.log(`WebContainer command routing passed for ${tested} commands, subcommands and aliases`)
}

function parseJsonOutput (output, command) {
  try {
    return JSON.parse(output)
  } catch (error) {
    throw new Error(`${command} returned invalid JSON: ${output}`, { cause: error })
  }
}

function aliasCommandPath (path) {
  const [canonicalPath, alias] = path.slice('alias:'.length).split('=')
  const parentPath = canonicalPath.split(' ').slice(0, -1).join(' ')
  return [parentPath, alias].filter(Boolean).join(' ')
}

function listCleanRemnants () {
  const root = 'node_modules/.pnpm'
  const remaining = []
  const visit = (directory, depth) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const relative = path.relative(root, path.join(directory, entry.name))
      remaining.push(`${entry.isSymbolicLink() ? 'link' : entry.isDirectory() ? 'dir' : 'file'} ${relative}`)
      if (entry.isDirectory() && depth < 3) visit(path.join(directory, entry.name), depth + 1)
    }
  }
  visit(root, 0)
  console.log(`clean left ${remaining.length} entries under node_modules/.pnpm:\n${remaining.slice(0, 80).join('\n')}`)
}

function createCleanTreeFixture () {
  const root = '/tmp/pnpm-clean-audit'
  const packageDirectory = path.join(root, 'node_modules/.pnpm/package/node_modules')
  const target = path.join(root, 'node_modules/.pnpm/target')
  mkdirSync(packageDirectory, { recursive: true })
  mkdirSync(target, { recursive: true })
  writeFileSync(path.join(root, 'package.json'), JSON.stringify({ name: 'clean-audit', version: '1.0.0' }))
  writeFileSync(path.join(target, 'file.txt'), 'clean me')
  symlinkSync(path.relative(packageDirectory, target), path.join(packageDirectory, 'target'), 'dir')
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
