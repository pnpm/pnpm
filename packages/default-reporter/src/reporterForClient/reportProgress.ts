import { CliLog, ProgressLog, StageLog } from '@pnpm/core-loggers'
import most = require('most')
import R = require('ramda')
import { hlValue } from './outputConstants'

export default (
  log$: {
    progress: most.Stream<ProgressLog>,
    cli: most.Stream<CliLog>,
    stage: most.Stream<StageLog>,
  },
  opts: {
    cmd: string,
    isRecursive: boolean,
    throttleProgress?: number,
  },
) => {
  const resolutionDone$ = opts.isRecursive
    ? most.never()
    : log$.stage
      .filter((log) => log.message === 'resolution_done')

  const resolvingContentLog$ = log$.progress
    .filter((log) => log.status === 'resolving_content')
    .scan(R.inc, 0)
    .skip(1)
    .until(resolutionDone$)

  const fedtchedLog$ = log$.progress
    .filter((log) => log.status === 'fetched')
    .scan(R.inc, 0)

  const foundInStoreLog$ = log$.progress
    .filter((log) => log.status === 'found_in_store')
    .scan(R.inc, 0)

  function createStatusMessage (resolving: number, fetched: number, foundInStore: number, importingDone: boolean) {
    const msg = `Resolving: total ${hlValue(resolving.toString())}, reused ${hlValue(foundInStore.toString())}, downloaded ${hlValue(fetched.toString())}`
    if (importingDone) {
      return {
        done: true,
        fixed: false,
        msg: `${msg}, done`,
      }
    }
    return {
      fixed: true,
      msg,
    }
  }

  const importingDone$ = (() => {
    if (opts.cmd === 'link') {
      return most.of(false)
    }
    const stageToWaitFor = opts.isRecursive ? 'recursive_importing_done' : 'importing_done'
    return log$.stage
      .filter((log) => log.message === stageToWaitFor)
      .constant(true)
      .take(1)
      .startWith(false)
      .multicast()
  })()

  if (typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0) {
    const resolutionStarted$ = log$.stage
      .filter((log) => log.message === 'resolution_started' || log.message === 'importing_started').take(1)
    const commandDone$ = log$.cli.filter((log) => log.message === 'command_done')

    // Reporting is done every `throttleProgress` milliseconds
    // and once all packages are fetched.
    const sampler = opts.isRecursive
      ? most.merge(most.periodic(opts.throttleProgress).until(commandDone$), commandDone$)
      : most.merge(
        most.periodic(opts.throttleProgress).since(resolutionStarted$).until(most.merge<{}>(importingDone$.skip(1), commandDone$)),
        importingDone$,
      )
    const progress = most.sample(
      createStatusMessage,
      sampler,
      resolvingContentLog$,
      fedtchedLog$,
      foundInStoreLog$,
      importingDone$,
    )
    // Avoid logs after all resolved packages were downloaded.
    // Fixing issue: https://github.com/pnpm/pnpm/issues/1028#issuecomment-364782901
    .skipAfter((msg) => msg.done === true)

    return most.of(progress)
  }
  const progress = most.combine(
    createStatusMessage,
    resolvingContentLog$,
    fedtchedLog$,
    foundInStoreLog$,
    opts.isRecursive ? most.of(false) : importingDone$,
  )
  return most.of(progress)
}
