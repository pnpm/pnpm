import fs from 'fs'
import path from 'path'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import audit from '@pnpm/audit'

async function writeResponse (lockfileDir: string, filename: string, opts: {
  production?: boolean
  dev?: boolean
  optional?: boolean
}) {
  const lockfile = await readWantedLockfile(lockfileDir, { ignoreIncompatible: true })
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const auditReport = await audit(lockfile!, {
    agentOptions: {},
    include,
    registry: 'https://registry.npmjs.org/',
  })
  fs.writeFileSync(path.join(__dirname, filename), JSON.stringify(auditReport, null, 2))
}

; (async () => {
  await writeResponse(path.join(__dirname, '../test/fixtures/has-vulnerabilities'), 'dev-vulnerabilities-only-response.json', {
    dev: true,
    production: false,
  })
  await writeResponse(path.join(__dirname, '../test/fixtures/has-vulnerabilities'), 'all-vulnerabilities-response.json', {})
  await writeResponse(path.join(__dirname, '../../../fixtures/has-outdated-deps'), 'no-vulnerabilities-response.json', {})
})()

