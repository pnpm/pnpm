import * as exec from './exec'
import * as restart from './restart'
import * as run from './run'
import * as _test from './test'

const test = {
  ...run,
  ..._test,
}

export { exec, restart, run, test }
