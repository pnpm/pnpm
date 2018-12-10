import chalk from 'chalk'
import most = require('most')
import * as supi from 'supi'
import { autozoom } from './utils/zooming'

export default (
  hook$: most.Stream<supi.Log>,
  opts: {
    cwd: string,
    isRecursive: boolean,
  }
) => {
  return hook$
    .map((log) => ({
      msg: autozoom(
        opts.cwd,
        log['prefix'],
        `${chalk.magentaBright(log['hook'])}: ${log['message']}`,
        {
          zoomOutCurrent: opts.isRecursive,
        },
      ),
    }))
    .map(most.of)
}
