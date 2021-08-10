import * as dlx from './dlx'
import * as exec from './exec'
import * as restart from './restart'
import * as run from './run'
import * as _test from './test'

const test = {
  ...run,
  ..._test,
}

export { dlx, exec, restart, run, test }
