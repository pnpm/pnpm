import most = require('most')
import * as supi from 'supi'

export default (
  skippedOptionalDependency$: most.Stream<supi.SkippedOptionalDependencyLog>,
  opts: {
    cwd: string,
  }
) => {
  return skippedOptionalDependency$
    .filter((log) => Boolean(log['prefix'] === opts.cwd && log.parents && log.parents.length === 0))
    .map((log) => most.of({
      msg: `info: ${
        log.package['id'] || log.package.name && (`${log.package.name}@${log.package.version}`) || log.package['pref']
      } is an optional dependency and failed compatibility check. Excluding it from installation.`,
    }))
}
