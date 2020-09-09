import { ProgressLog, StageLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, map, mapTo, sampleTime, takeWhile, startWith, take } from 'rxjs/operators'
import { hlValue } from './outputConstants'
import { zoomOut } from './utils/zooming'

interface ProgressStats {
  fetched: number
  resolved: number
  reused: number
}

interface ModulesInstallProgress {
  importingDone$: Rx.Observable<boolean>
  progress$: Rx.Observable<ProgressStats>
  requirer: string
}

export default (
  log$: {
    progress: Rx.Observable<ProgressLog>
    stage: Rx.Observable<StageLog>
  },
  opts: {
    cwd: string
    throttleProgress?: number
  }
) => {
  const progressOutput = typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0
    ? throttledProgressOutput.bind(null, opts.throttleProgress)
    : nonThrottledProgressOutput

  return getModulesInstallProgress$(log$.stage, log$.progress).pipe(
    map(({ importingDone$, progress$, requirer }) => {
      const output$ = progressOutput(importingDone$, progress$)

      if (requirer === opts.cwd) {
        return output$
      }
      return output$.pipe(
        map((msg: any) => { // eslint-disable-line @typescript-eslint/no-explicit-any
          msg['msg'] = zoomOut(opts.cwd, requirer, msg['msg'])
          return msg
        })
      )
    })
  )
}

function throttledProgressOutput (
  throttleProgress: number,
  importingDone$: Rx.Observable<boolean>,
  progress$: Rx.Observable<ProgressStats>
) {
  // Reporting is done every `throttleProgress` milliseconds
  // and once all packages are fetched.
  return Rx.combineLatest(
    progress$.pipe(sampleTime(throttleProgress)),
    importingDone$
  )
    .pipe(
      map(createStatusMessage),
      // Avoid logs after all resolved packages were downloaded.
      // Fixing issue: https://github.com/pnpm/pnpm/issues/1028#issuecomment-364782901
      takeWhile((msg) => msg['done'] !== true, true)
    )
}

function nonThrottledProgressOutput (
  importingDone$: Rx.Observable<boolean>,
  progress$: Rx.Observable<ProgressStats>
) {
  return Rx.combineLatest(
    progress$,
    importingDone$
  )
    .pipe(map(createStatusMessage))
}

function getModulesInstallProgress$ (
  stage$: Rx.Observable<StageLog>,
  progress$: Rx.Observable<ProgressLog>
): Rx.Observable<ModulesInstallProgress> {
  const modulesInstallProgressPushStream = new Rx.Subject<ModulesInstallProgress>()
  const progessStatsPushStreamByRequirer = getProgessStatsPushStreamByRequirer(progress$)

  const stagePushStreamByRequirer: {
    [requirer: string]: Rx.Subject<StageLog>
  } = {}
  stage$
    .forEach((log: StageLog) => {
      if (!stagePushStreamByRequirer[log.prefix]) {
        stagePushStreamByRequirer[log.prefix] = new Rx.Subject<StageLog>()
        if (!progessStatsPushStreamByRequirer[log.prefix]) {
          progessStatsPushStreamByRequirer[log.prefix] = new Rx.Subject()
        }
        modulesInstallProgressPushStream.next({
          importingDone$: stage$ToImportingDone$(Rx.from(stagePushStreamByRequirer[log.prefix])),
          progress$: Rx.from(progessStatsPushStreamByRequirer[log.prefix]),
          requirer: log.prefix,
        })
      }
      stagePushStreamByRequirer[log.prefix].next(log)
      if (log.stage === 'importing_done') {
        progessStatsPushStreamByRequirer[log.prefix].complete()
        stagePushStreamByRequirer[log.prefix].complete()
      }
    })
    .catch(() => {})

  return Rx.from(modulesInstallProgressPushStream)
}

function stage$ToImportingDone$ (stage$: Rx.Observable<StageLog>) {
  return stage$
    .pipe(
      filter((log: StageLog) => log.stage === 'importing_done'),
      mapTo(true),
      take(1),
      startWith(false)
    )
}

function getProgessStatsPushStreamByRequirer (progress$: Rx.Observable<ProgressLog>) {
  const progessStatsPushStreamByRequirer: {
    [requirer: string]: Rx.Subject<ProgressStats>
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
      if (!progessStatsPushStreamByRequirer[log.requester]) {
        progessStatsPushStreamByRequirer[log.requester] = new Rx.Subject<ProgressStats>()
      }
      progessStatsPushStreamByRequirer[log.requester].next(previousProgressStatsByRequirer[log.requester])
    })
    .catch(() => {})

  return progessStatsPushStreamByRequirer
}

function createStatusMessage ([progress, importingDone]: [ProgressStats, boolean]) {
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
