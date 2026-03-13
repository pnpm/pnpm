import { prepare } from '@pnpm/prepare'

import { execPnpm, execPnpmSync } from './utils/index.js'

test('pnpm sbom --sbom-format cyclonedx outputs valid JSON to stdout', async () => {
  prepare({
    dependencies: {
      'is-positive': '3.1.0',
    },
  })
  await execPnpm(['install'])

  const { status, stdout } = execPnpmSync(['sbom', '--sbom-format', 'cyclonedx'])

  expect(status).toBe(0)

  const parsed = JSON.parse(stdout.toString())
  expect(parsed.bomFormat).toBe('CycloneDX')
  expect(parsed.specVersion).toBe('1.7')
  expect(parsed.components.length).toBeGreaterThan(0)
})

test('pnpm sbom --sbom-format spdx outputs valid JSON to stdout', async () => {
  prepare({
    dependencies: {
      'is-positive': '3.1.0',
    },
  })
  await execPnpm(['install'])

  const { status, stdout } = execPnpmSync(['sbom', '--sbom-format', 'spdx'])

  expect(status).toBe(0)

  const parsed = JSON.parse(stdout.toString())
  expect(parsed.spdxVersion).toBe('SPDX-2.3')
  expect(parsed.dataLicense).toBe('CC0-1.0')
})

test('pnpm sbom warnings go to stderr, not stdout', async () => {
  prepare({
    dependencies: {
      'is-positive': '3.1.0',
    },
  })
  await execPnpm(['install'])

  // pnpm_config_force triggers a WARN log; disable silent mode so the reporter runs
  const { status, stdout, stderr } = execPnpmSync(
    ['sbom', '--sbom-format', 'cyclonedx'],
    { env: { pnpm_config_silent: 'false', pnpm_config_force: 'true' } }
  )

  expect(status).toBe(0)

  // stdout must still be valid JSON
  const parsed = JSON.parse(stdout.toString())
  expect(parsed.bomFormat).toBe('CycloneDX')

  // the --force warning should be on stderr, not stdout
  expect(stderr.toString()).toContain('using --force')
})
