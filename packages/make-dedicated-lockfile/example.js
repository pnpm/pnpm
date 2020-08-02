const audit = require('./lib').default
const { readWantedLockfile } = require('@pnpm/lockfile-file')

readWantedLockfile('../..', {})
  .then((lockfile) => audit(lockfile, { registry: 'https://registry.npmjs.org' }))
  .then((auditResult) => console.log(JSON.stringify(auditResult, null, 2)))
  .catch(console.log.bind(console))
