import { ProgressLog, StageLog } from '@pnpm/core-loggers'
import PushStream from '@zkochan/zen-push'
import most = require('most')
import { hlValue } from './outputConstants'
import { zoomOut } from './utils/zooming'

type ProgressStats = {
  fetched: number,
  resolved: number,
  reused: number,
}

type ModulesInstallProgress = {
  progress$: most.Stream<ProgressStats>,
  requirer: string,
  stage$: most.Stream<StageLog>,
}

export default (
  log$: {
    progress: most.Stream<ProgressLog>,
    stage: most.Stream<StageLog>,
  },
  opts: {
    cwd: string,
    throttleProgress?: number,
  },
) => {
  const modulesInstallProgressPushStream = getModulesInstallProgressPushStream(log$.stage, log$.progress)

  const progressOutput = typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0
    ? throttledProgressOutput.bind(null, opts.throttleProgress)
    : nonThrottledProgressOutput

  return most.from(modulesInstallProgressPushStream.observable)
    .map(({ stage$, progress$, requirer }: ModulesInstallProgress) => {
      const importingDone$ = stage$
        .filter((log: StageLog) => log.stage === 'importing_done')
        .constant(true)
        .take(1)
        .startWith(false)
        .multicast()

      const output$ = progressOutput(importingDone$, progress$)

      if (requirer === opts.cwd) {
        return output$
      }
      return output$.map((msg: any) => { // tslint:disable-line:no-any
        msg['msg'] = zoomOut(opts.cwd, requirer, msg['msg'])
        return msg
      })
    })
}

function throttledProgressOutput (
  throttleProgress: number,
  importingDone$: most.Stream<boolean>,
  progress$: most.Stream<ProgressStats>,
) {
  // Reporting is done every `throttleProgress` milliseconds
  // and once all packages are fetched.
  const sampler = most.merge(
    most.periodic(throttleProgress).until(importingDone$),
    importingDone$,
  )
  return most.sample(
    createStatusMessage,
    sampler,
    progress$,
    importingDone$,
  )
  // Avoid logs after all resolved packages were downloaded.
  // Fixing issue: https://github.com/pnpm/pnpm/issues/1028#issuecomment-364782901
  .skipAfter((msg) => msg['done'] === true)
}

function nonThrottledProgressOutput (
  importingDone$: most.Stream<boolean>,
  progress$: most.Stream<ProgressStats>,
) {
  return most.combine(
    createStatusMessage,
    progress$,
    importingDone$,
  )
}

function getModulesInstallProgressPushStream (
  stage$: most.Stream<StageLog>,
  progress$: most.Stream<ProgressLog>,
) {
  const modulesInstallProgressPushStream = new PushStream<ModulesInstallProgress>()
  const progessStatsPushStreamByRequirer = getProgessStatsPushStreamByRequirer(progress$)

  const stagePushStreamByRequirer: {
    [requirer: string]: PushStream<StageLog>,
  } = {}
  stage$
    .forEach((log: StageLog) => {
      if (!stagePushStreamByRequirer[log.prefix]) {
        stagePushStreamByRequirer[log.prefix] = new PushStream<StageLog>()
        if (!progessStatsPushStreamByRequirer[log.prefix]) {
          progessStatsPushStreamByRequirer[log.prefix] = new PushStream()
        }
        modulesInstallProgressPushStream.next({
          progress$: most.from(progessStatsPushStreamByRequirer[log.prefix].observable),
          requirer: log.prefix,
          stage$: most.from(stagePushStreamByRequirer[log.prefix].observable),
        })
      }
      stagePushStreamByRequirer[log.prefix].next(log)
      if (log.stage === 'importing_done') {
        progessStatsPushStreamByRequirer[log.prefix].complete()
        stagePushStreamByRequirer[log.prefix].complete()
      }
    })

  return modulesInstallProgressPushStream
}

function getProgessStatsPushStreamByRequirer (progress$: most.Stream<ProgressLog>) {
  const progessStatsPushStreamByRequirer: {
    [requirer: string]: PushStream<ProgressStats>,
  } = {}

  const previousProgressStatsByRequirer: { [requirer: string]: ProgressStats } = {}
  progress$
    .forEach((log: ProgressLog) => {
      if (!previousProgressStatsByRequirer[log.requester]) {
        previousProgressStatsByRequirer[log.requester] = {
          fetched: 0,
          resolved: 0,
          reused: 0,
        }
      } else {
        previousProgressStatsByRequirer[log.requester] = {
          ...previousProgressStatsByRequirer[log.requester]
        }
      }
      switch (log.status) {
        case 'resolved':
          previousProgressStatsByRequirer[log.requester].resolved++
          break
        case 'fetched':
          previousProgressStatsByRequirer[log.requester].fetched++
          break
        case 'found_in_store':
          previousProgressStatsByRequirer[log.requester].reused++
          break
      }
      progessStatsPushStreamByRequirer[log.requester] = progessStatsPushStreamByRequirer[log.requester] || new PushStream<ProgressLog>()
      progessStatsPushStreamByRequirer[log.requester].next(previousProgressStatsByRequirer[log.requester])
    })

  return progessStatsPushStreamByRequirer
}

function createStatusMessage (
  progress: ProgressStats,
  importingDone: boolean,
) {
  const msg = `Resolving: total ${hlValue(progress.resolved.toString())}, reused ${hlValue(progress.reused.toString())}, downloaded ${hlValue(progress.fetched.toString())}`
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
