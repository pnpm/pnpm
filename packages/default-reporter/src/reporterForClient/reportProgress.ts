import { ProgressLog, StageLog } from '@pnpm/core-loggers'
import * as Rx from 'rxjs'
import { filter, map, mapTo, takeWhile, startWith, take } from 'rxjs/operators'
import { hlValue } from './outputConstants'
import { zoomOut } from './utils/zooming'

interface ProgressStats {
  fetched: number
  imported: number
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
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    throttle?: Rx.OperatorFunction<any, any>
  }
) => {
  const progressOutput = throttledProgressOutput.bind(null, opts.throttle)

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
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  throttle: Rx.OperatorFunction<any, any> | undefined,
  importingDone$: Rx.Observable<boolean>,
  progress$: Rx.Observable<ProgressStats>
) {
  let combinedProgress = Rx.combineLatest(
    progress$,
    importingDone$
  )
    // Avoid logs after all resolved packages were downloaded.
    // Fixing issue: https://github.com/pnpm/pnpm/issues/1028#issuecomment-364782901
    .pipe(takeWhile(([, importingDone]) => !importingDone, true))
  if (throttle) {
    combinedProgress = combinedProgress.pipe(throttle)
  }
  return combinedProgress.pipe(map(createStatusMessage))
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
          imported: 0,
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
      case 'imported':
        previousProgressStatsByRequirer[log.requester].imported++
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
  const msg = `Progress: resolved ${hlValue(progress.resolved.toString())}, reused ${hlValue(progress.reused.toString())}, downloaded ${hlValue(progress.fetched.toString())}, added ${hlValue(progress.imported.toString())}`
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
