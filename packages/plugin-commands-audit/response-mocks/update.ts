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
  await writeResponse(path.join(__dirname, '../test/fixtures/has-vulnerabilities'), 'response1.json', {
    dev: true,
    production: false,
  })
  await writeResponse(path.join(__dirname, '../test/fixtures/has-vulnerabilities'), 'response2.json', {})
  await writeResponse(path.join(__dirname, '../../../fixtures/has-outdated-deps'), 'response3.json', {})
})()

