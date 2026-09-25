import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { prepare, preparePackages } from '@pnpm/prepare'
import { readYamlFileSync } from 'read-yaml-file'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

// The public npm registry is used here instead of verdaccio because the
// registry mock doesn't include the per-version `time` field in full-metadata
// responses, which the lockfile verifier needs to evaluate the cutoff.
// This mirrors the workaround in `pnpm/test/dlx.ts`.
const PUBLIC_REGISTRY = '--config.registry=https://registry.npmjs.org/'

// `is-odd@0.1.2` was published in 2017. Setting an extreme minimumReleaseAge
// (~27 years) ensures every locked version is "immature" relative to the
// cutoff — the verifier rejects the entry regardless of when the test runs.
const IMMATURE_FOR_EVERYTHING = 60 * 24 * 365 * 27

// execPnpm's createEnv defaults pnpm_config_minimum_release_age to '0',
// which overrides anything in pnpm-workspace.yaml. Tests that need the
// yaml policy to take effect must omit this default — same workaround
// dlx.ts uses for its minimumReleaseAge tests.
const omitMinReleaseAgeEnv = { omitEnvDefaults: ['pnpm_config_minimum_release_age' as const] }

describe('lockfile minimumReleaseAge verification', () => {
  test('install rejects a lockfile entry that does not satisfy the policy in strict mode', async () => {
    // Step 1: populate a lockfile under no policy. The resolver picks
    // is-odd@0.1.2 (latest 0.1.x) without applying any maturity filter.
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    await execPnpm([PUBLIC_REGISTRY, 'install'])

    // Step 2: turn on minimumReleaseAge in strict mode. The lockfile is now
    // "poisoned" relative to the new policy — exactly the scenario the
    // verifier exists to catch (a teammate committed a lockfile that
    // bypassed the policy locally, a CI cache restored a stale lockfile,
    // etc.).
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: true,
    })

    const result = execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
      omitMinReleaseAgeEnv
    )

    expect(result.status).toBe(1)
    const output = `${result.stdout.toString()}\n${result.stderr.toString()}`
    expect(output).toContain('ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION')
    expect(output).toMatch(/is-odd@0\.1\.2/)
    // Confirm the recovery hint reaches the user.
    expect(output).toContain('pnpm clean --lockfile')
  })

  test('install respects minimumReleaseAgeExclude during lockfile verification', () => {
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    execPnpmSync([PUBLIC_REGISTRY, 'install'], { expectSuccess: true })

    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: true,
      // is-odd@0.1.2 pulls in is-buffer, is-number, and kind-of transitively;
      // all four are immature in this test, so all four need exclusion.
      minimumReleaseAgeExclude: ['is-odd', 'is-buffer', 'is-number', 'kind-of'],
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )
  })

  test('records the verification cache so a repeat install reuses it', async () => {
    // Step 1: populate the lockfile with no policy. is-positive@1.0.0
    // was published in 2014, so a 1-minute cutoff later will pass it.
    prepare({
      dependencies: { 'is-positive': '1.0.0' },
    })
    await execPnpm([PUBLIC_REGISTRY, 'install'])

    // Step 2: turn the policy on. The post-resolution gate now runs
    // against the existing lockfile and writes a cache record.
    const cacheDir = path.resolve('pnpm-cache')
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: 1,
      minimumReleaseAgeStrict: true,
      cacheDir,
    })
    execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )

    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    expect(fs.existsSync(cacheFile)).toBe(true)
    const lines = fs.readFileSync(cacheFile, 'utf8').split('\n').filter(Boolean)
    expect(lines.length).toBeGreaterThanOrEqual(1)
    const record = JSON.parse(lines.at(-1)!) as {
      lockfile: { hash: string, path: string }
      policy: Record<string, unknown>
    }
    expect(record.lockfile.path).toBe(path.resolve('pnpm-lock.yaml'))
    expect(record.lockfile.hash).toMatch(/^[a-z0-9+/=]+$/i)
    expect(record.policy).toMatchObject({ minimumReleaseAge: 1 })

    // Step 3: another install with the same lockfile + policy. The cache
    // short-circuits the gate (asserting that requires registry-call
    // instrumentation we don't have at this layer, but install
    // completing cleanly is the smoke test).
    execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )
  })

  test('a fresh install records the just-written lockfile in the verification cache', async () => {
    // Reproduces the "install foo, rm -rf node_modules, install" flow:
    // the lockfile written by the first install must be recorded under
    // its post-resolution content, otherwise the second install re-runs
    // the registry round-trip even though the resolver already enforced
    // the policy when picking those versions.
    prepare({})
    const cacheDir = path.resolve('pnpm-cache')
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: 1,
      minimumReleaseAgeStrict: true,
      cacheDir,
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, 'add', 'is-positive@1.0.0'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )

    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    expect(fs.existsSync(cacheFile)).toBe(true)
    const lines = fs.readFileSync(cacheFile, 'utf8').split('\n').filter(Boolean)
    const records = lines.map((line) => JSON.parse(line) as {
      lockfile: { hash: string, path: string }
      policy: Record<string, unknown>
    })
    const lockfilePath = path.resolve('pnpm-lock.yaml')
    const recordForLockfile = records.find((r) => r.lockfile.path === lockfilePath)
    expect(recordForLockfile).toBeDefined()
    expect(recordForLockfile!.policy).toMatchObject({ minimumReleaseAge: 1 })

    // Re-running install completes without hitting the registry to
    // re-verify is-positive. We can't directly observe the network skip,
    // but a clean run with --offline confirms the cache short-circuit
    // works end-to-end (the verifier would otherwise need a registry
    // round-trip to evaluate the cutoff).
    execPnpmSync(
      [PUBLIC_REGISTRY, '--offline', 'install', '--frozen-lockfile'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )
  })

  test('loose mode rejects immature lockfile entries that are not on minimumReleaseAgeExclude', () => {
    // The verifier now runs in loose mode too, so a lockfile produced under
    // no policy that still has immature pins is rejected the same way
    // strict mode would reject it. The expected workflow is: the loose-mode
    // auto-collect (during fresh resolution) populates the exclude list, and
    // subsequent installs run cleanly against that list.
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    execPnpmSync([PUBLIC_REGISTRY, 'install'], { expectSuccess: true })

    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: false,
    })

    const result = execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
      omitMinReleaseAgeEnv
    )
    const output = `${result.stdout.toString()}\n${result.stderr.toString()}`
    expect(output).toContain('ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION')
    expect(output).toMatch(/is-odd@0\.1\.2/)
    expect(result.status).toBe(1)
  })

  test('loose mode auto-adds fresh immature picks to minimumReleaseAgeExclude', () => {
    // Fresh resolution under loose mode: the resolver's lowest-version
    // fallback picks an immature version, and the install layer surfaces it
    // to `minimumReleaseAgeExclude`. The verifier sees an empty lockfile at
    // the start (no entries to reject) and the workspace manifest grows.
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: false,
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, 'install'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )

    const workspaceManifest = readYamlFileSync<{ minimumReleaseAgeExclude?: string[] }>('pnpm-workspace.yaml')
    // is-odd@0.1.2 pulls in is-buffer, is-number, and kind-of transitively;
    // every one of those is immature relative to the (deliberately extreme)
    // cutoff, so all four end up on the exclude list. Match by package name
    // (any version) so the test stays stable across npm-registry republishes
    // that shift the transitive pins.
    expect(workspaceManifest.minimumReleaseAgeExclude).toEqual(expect.arrayContaining([
      'is-odd@0.1.2',
      expect.stringMatching(/^is-buffer@/),
      expect.stringMatching(/^is-number@/),
      expect.stringMatching(/^kind-of@/),
    ]))
  })

  test('loose-mode auto-exclude is a no-op when no immature picks occur', () => {
    // is-positive@1.0.0 was published in 2014; with a 1-minute cutoff it
    // stays mature relative to the policy. Auto-exclude should not touch
    // the workspace manifest when there's nothing to add.
    prepare({
      dependencies: { 'is-positive': '1.0.0' },
    })
    execPnpmSync([PUBLIC_REGISTRY, 'install'], { expectSuccess: true })

    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: 1,
      minimumReleaseAgeStrict: false,
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, 'install'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )

    const workspaceManifest = readYamlFileSync<{ minimumReleaseAgeExclude?: string[] }>('pnpm-workspace.yaml')
    expect(workspaceManifest.minimumReleaseAgeExclude).toBeUndefined()
  })

  test('subsequent installs run cleanly once the exclude list is populated', () => {
    // Round-trip the auto-collect: first install populates the exclude list
    // from fresh resolution, the next install runs the verifier against the
    // now-populated list and succeeds without re-announcing anything. The
    // verifier and the auto-collect together keep the workspace manifest in
    // sync with the lockfile across installs.
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: false,
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, 'install'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )

    execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )
  })

  test('recursive --no-save leaves the workspace manifest untouched even when picks are collected (shared lockfile)', () => {
    // The shared-lockfile recursive branch in recursive.ts: a single
    // `mutateModules` call across all importers. Same drain-only-when-
    // saving gate has to hold here.
    preparePackages([
      {
        name: 'project-a',
        version: '1.0.0',
        dependencies: { 'is-odd': '0.1.2' },
      },
    ])
    writeYamlFileSync('pnpm-workspace.yaml', {
      packages: ['*'],
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: false,
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, '-r', 'install', '--no-save'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )

    const workspaceManifest = readYamlFileSync<{ minimumReleaseAgeExclude?: string[] }>('pnpm-workspace.yaml')
    expect(workspaceManifest.minimumReleaseAgeExclude).toBeUndefined()
  })

  test('recursive --no-save leaves the workspace manifest untouched even when picks are collected (per-project lockfiles)', () => {
    // The other recursive branch: with sharedWorkspaceLockfile: false
    // the per-project loop is taken instead of the single
    // mutateModules call. The post-loop updateWorkspaceManifest at the
    // tail of recursive.ts also has to honor --no-save.
    preparePackages([
      {
        name: 'project-a',
        version: '1.0.0',
        dependencies: { 'is-odd': '0.1.2' },
      },
    ])
    writeYamlFileSync('pnpm-workspace.yaml', {
      packages: ['*'],
      sharedWorkspaceLockfile: false,
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: false,
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, '-r', 'install', '--no-save'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )

    const workspaceManifest = readYamlFileSync<{ minimumReleaseAgeExclude?: string[] }>('pnpm-workspace.yaml')
    expect(workspaceManifest.minimumReleaseAgeExclude).toBeUndefined()
  })

  test('--no-save leaves the workspace manifest untouched even when picks are collected', () => {
    // `--no-save` means "don't persist anything from this install" — the
    // auto-add should obey that. Without the gate, the info log would
    // claim entries were added that never reached pnpm-workspace.yaml,
    // and the next install would either re-prompt or fail verification.
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: false,
    })

    // First install resolves and populates the lockfile but not the
    // workspace manifest (because --no-save).
    execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--no-save'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )

    const workspaceManifest = readYamlFileSync<{ minimumReleaseAgeExclude?: string[] }>('pnpm-workspace.yaml')
    expect(workspaceManifest.minimumReleaseAgeExclude).toBeUndefined()
  })

  test('verifier cache invalidates when minimumReleaseAgeExclude is shrunk', async () => {
    // Removing an entry from the exclude list could expose a violation
    // that previously passed verification. The cache record snapshots the
    // exclude list and `canTrustPastCheck` rejects the cached run when
    // today's list isn't a superset of the cached one — so the next
    // install re-verifies and the now-uncovered immature lockfile entry
    // is flagged.
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    await execPnpm([PUBLIC_REGISTRY, 'install'])

    const cacheDir = path.resolve('pnpm-cache')
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: true,
      minimumReleaseAgeExclude: ['is-odd', 'is-buffer', 'is-number', 'kind-of'],
      cacheDir,
    })
    // Step 1: install with the full exclude list — verifier writes a
    // cache record under that policy.
    execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )
    const cacheFile = path.join(cacheDir, 'lockfile-verified.jsonl')
    expect(fs.existsSync(cacheFile)).toBe(true)

    // Step 2: drop `is-odd` from the exclude list. The cached record
    // had it; today doesn't. canTrustPastCheck must reject so the
    // re-verification flags is-odd@0.1.2 as immature.
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: true,
      minimumReleaseAgeExclude: ['is-buffer', 'is-number', 'kind-of'],
      cacheDir,
    })
    const result = execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
      omitMinReleaseAgeEnv
    )
    expect(result.status).toBe(1)
    const output = `${result.stdout.toString()}\n${result.stderr.toString()}`
    expect(output).toContain('ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION')
    expect(output).toMatch(/is-odd@0\.1\.2/)
  })

  test('--fix-lockfile enforces minimumReleaseAge on existing lockfile entries', () => {
    // Reproduces the scenario from https://github.com/pnpm/pnpm/issues/10361:
    // a lockfile entry that was installed while the package was immature (possibly
    // under a minimumReleaseAgeExclude) must still be rejected by the verifier when
    // --fix-lockfile is run without an exclude covering it. The fix-lockfile path
    // must not bypass the lockfile verification step.
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    execPnpmSync([PUBLIC_REGISTRY, 'install'], { expectSuccess: true })

    // Turn on an extreme minimumReleaseAge (no exclude list) so every locked
    // entry is considered immature.
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: true,
    })

    const result = execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--fix-lockfile'],
      omitMinReleaseAgeEnv
    )

    expect(result.status).toBe(1)
    const output = `${result.stdout.toString()}\n${result.stderr.toString()}`
    expect(output).toContain('ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION')
    expect(output).toMatch(/is-odd@0\.1\.2/)
  })

  test('--fix-lockfile respects minimumReleaseAgeExclude for entries in the exclude list', () => {
    // When minimumReleaseAgeExclude covers the immature lockfile entries, --fix-lockfile
    // should succeed (the verifier accepts excluded entries).
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    execPnpmSync([PUBLIC_REGISTRY, 'install'], { expectSuccess: true })

    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: true,
      // is-odd is version-qualified to exercise the exact-version exclude path
      // (the `name@version` form that issue 10361 relies on); the transitive
      // deps stay name-only because their resolved versions shift across npm
      // republishes.
      minimumReleaseAgeExclude: ['is-odd@0.1.2', 'is-buffer', 'is-number', 'kind-of'],
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--fix-lockfile'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )
  })
})
