import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
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

  test('loose mode lets immature lockfile entries install and auto-adds them to minimumReleaseAgeExclude', () => {
    // The config reader auto-enables strict mode the moment a user
    // explicitly sets `minimumReleaseAge`, so opting out requires an
    // explicit `minimumReleaseAgeStrict: false`. With that, the verifier
    // doesn't throw — but the install layer still surfaces the immature
    // picks so they can be persisted to the workspace manifest's exclude
    // list, making the loose-mode bypass explicit on subsequent installs.
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    execPnpmSync([PUBLIC_REGISTRY, 'install'], { expectSuccess: true })

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

  test('loose-mode auto-exclude preserves entries the user already declared', () => {
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    execPnpmSync([PUBLIC_REGISTRY, 'install'], { expectSuccess: true })

    // Pre-existing user entry must survive the auto-merge; the writer
    // dedupes against existing values so 'kind-of' is not duplicated.
    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: false,
      minimumReleaseAgeExclude: ['kind-of'],
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, 'install'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )

    const workspaceManifest = readYamlFileSync<{ minimumReleaseAgeExclude?: string[] }>('pnpm-workspace.yaml')
    const excludes = workspaceManifest.minimumReleaseAgeExclude ?? []
    expect(excludes).toContain('kind-of')
    expect(excludes).toContain('is-odd@0.1.2')
    // No duplicates introduced by the auto-merge.
    expect(excludes.length).toBe(new Set(excludes).size)
  })
})
