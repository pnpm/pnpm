import { HookLog } from '@pnpm/core-loggers'
import chalk = require('chalk')
import most = require('most')
import { autozoom } from './utils/zooming'

export default (
  hook$: most.Stream<HookLog>,
  opts: {
    cwd: string,
    isRecursive: boolean,
  }
) => {
  return hook$
    .map((log) => ({
      msg: autozoom(
        opts.cwd,
        log.prefix,
        `${chalk.magentaBright(log.hook)}: ${log.message}`,
        {
          zoomOutCurrent: opts.isRecursive,
        },
      ),
    }))
    .map(most.of)
}
