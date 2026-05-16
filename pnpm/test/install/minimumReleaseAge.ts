import { describe, expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
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

  test('install is unaffected by minimumReleaseAge when strict mode is explicitly off', () => {
    // The config reader auto-enables strict mode the moment a user
    // explicitly sets `minimumReleaseAge`, so opting out requires an
    // explicit `minimumReleaseAgeStrict: false`. With that, the verifier
    // doesn't construct and the lockfile passes through untouched.
    prepare({
      dependencies: { 'is-odd': '0.1.2' },
    })
    execPnpmSync([PUBLIC_REGISTRY, 'install'], { expectSuccess: true })

    writeYamlFileSync('pnpm-workspace.yaml', {
      minimumReleaseAge: IMMATURE_FOR_EVERYTHING,
      minimumReleaseAgeStrict: false,
    })

    execPnpmSync(
      [PUBLIC_REGISTRY, 'install', '--frozen-lockfile'],
      { ...omitMinReleaseAgeEnv, expectSuccess: true }
    )
  })
})
