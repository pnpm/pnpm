import * as Rx from 'rxjs'
import { filter, map, mapTo, takeWhile, startWith, take } from 'rxjs/operators'

import { ProgressLog, StageLog } from '@pnpm/types'

import { zoomOut } from './utils/zooming'
import { hlValue } from './outputConstants'

type ProgressStats = {
  fetched: number
  imported: number
  resolved: number
  reused: number
}

type ModulesInstallProgress = {
  importingDone$: Rx.Observable<boolean>
  progress$: Rx.Observable<ProgressStats>
  requirer: string
}

export function reportProgress(
  log$: {
    progress: Rx.Observable<ProgressLog>
    stage: Rx.Observable<StageLog>
  },
  opts: {
    cwd: string
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    throttle?: Rx.OperatorFunction<any, any>
    hideAddedPkgsProgress?: boolean
    hideProgressPrefix?: boolean
  }
) {
  const progressOutput = throttledProgressOutput.bind(null, opts)

  return getModulesInstallProgress$(log$.stage, log$.progress).pipe(
    map(
      opts.hideProgressPrefix === true
        ? ({ importingDone$, progress$ }: ModulesInstallProgress) => {
          return progressOutput(importingDone$, progress$)
        }
        : ({ importingDone$, progress$, requirer }: ModulesInstallProgress) => {
          const output$ = progressOutput(importingDone$, progress$)

          if (requirer === opts.cwd) {
            return output$
          }
          return output$.pipe(
            map((msg) => {
              msg.msg = zoomOut(opts.cwd, requirer, msg.msg)
              return msg
            })
          )
        }
    )
  )
}

function throttledProgressOutput(
  opts: {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    throttle?: Rx.OperatorFunction<any, any>
    hideAddedPkgsProgress?: boolean
  },
  importingDone$: Rx.Observable<boolean>,
  progress$: Rx.Observable<ProgressStats>
) {
  if (opts.throttle != null) {
    progress$ = progress$.pipe(opts.throttle)
  }
  const combinedProgress = Rx.combineLatest(progress$, importingDone$)
    // Avoid logs after all resolved packages were downloaded.
    // Fixing issue: https://github.com/pnpm/pnpm/issues/1028#issuecomment-364782901
    .pipe(takeWhile(([, importingDone]) => !importingDone, true))
  return combinedProgress.pipe(
    map(
      opts.hideAddedPkgsProgress
        ? createStatusMessageWithoutAdded
        : createStatusMessage
    )
  )
}

function getModulesInstallProgress$(
  stage$: Rx.Observable<StageLog>,
  progress$: Rx.Observable<ProgressLog>
): Rx.Observable<ModulesInstallProgress> {
  const modulesInstallProgressPushStream =
    new Rx.Subject<ModulesInstallProgress>()
  const progressStatsPushStreamByRequirer =
    getProgressStatsPushStreamByRequirer(progress$)

  const stagePushStreamByRequirer: {
    [requirer: string]: Rx.Subject<StageLog>
  } = {}
  stage$
    .forEach((log: StageLog) => {
      if (!stagePushStreamByRequirer[log.prefix]) {
        stagePushStreamByRequirer[log.prefix] = new Rx.Subject<StageLog>()
        if (!progressStatsPushStreamByRequirer[log.prefix]) {
          progressStatsPushStreamByRequirer[log.prefix] = new Rx.Subject()
        }
        modulesInstallProgressPushStream.next({
          importingDone$: stage$ToImportingDone$(
            Rx.from(stagePushStreamByRequirer[log.prefix])
          ),
          progress$: Rx.from(progressStatsPushStreamByRequirer[log.prefix]),
          requirer: log.prefix,
        })
      }
      stagePushStreamByRequirer[log.prefix].next(log)
      if (log.stage === 'importing_done') {
        progressStatsPushStreamByRequirer[log.prefix].complete()
        stagePushStreamByRequirer[log.prefix].complete()
      }
    })
    .catch(() => {})

  return Rx.from(modulesInstallProgressPushStream)
}

function stage$ToImportingDone$(stage$: Rx.Observable<StageLog>): Rx.Observable<boolean> {
  return stage$.pipe(
    filter((log: StageLog) => log.stage === 'importing_done'),
    mapTo(true),
    take(1),
    startWith(false)
  )
}

function getProgressStatsPushStreamByRequirer(
  progress$: Rx.Observable<ProgressLog>
): Record<string, Rx.Subject<ProgressStats>> {
  const progressStatsPushStreamByRequirer: Record<string, Rx.Subject<ProgressStats>> = {}

  const previousProgressStatsByRequirer: Record<string, ProgressStats> =
    {}

  progress$
    .forEach((log: ProgressLog): void => {
      if (!previousProgressStatsByRequirer[log.requester]) {
        previousProgressStatsByRequirer[log.requester] = {
          fetched: 0,
          imported: 0,
          resolved: 0,
          reused: 0,
        }
      }
      switch (log.status) {
        case 'resolved': {
          previousProgressStatsByRequirer[log.requester].resolved++
          break
        }
        case 'fetched': {
          previousProgressStatsByRequirer[log.requester].fetched++
          break
        }
        case 'found_in_store': {
          previousProgressStatsByRequirer[log.requester].reused++
          break
        }
        case 'imported': {
          previousProgressStatsByRequirer[log.requester].imported++
          break
        }
      }

      if (!progressStatsPushStreamByRequirer[log.requester]) {
        progressStatsPushStreamByRequirer[log.requester] =
          new Rx.Subject<ProgressStats>()
      }

      progressStatsPushStreamByRequirer[log.requester].next(
        previousProgressStatsByRequirer[log.requester]
      )
    })
    .catch((err) => {
      console.error(err)
    })

  return progressStatsPushStreamByRequirer
}

function createStatusMessage([progress, importingDone]: [
  ProgressStats,
  boolean,
]) {
  const msg = `Progress: resolved ${hlValue(
    progress.resolved.toString()
  )}, reused ${hlValue(progress.reused.toString())}, downloaded ${hlValue(
    progress.fetched.toString()
  )}, added ${hlValue(progress.imported.toString())}`

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

function createStatusMessageWithoutAdded([progress, importingDone]: [
  ProgressStats,
  boolean,
]) {
  const msg = `Progress: resolved ${hlValue(
    progress.resolved.toString()
  )}, reused ${hlValue(progress.reused.toString())}, downloaded ${hlValue(
    progress.fetched.toString()
  )}`

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
