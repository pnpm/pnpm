import * as Rx from 'rxjs'
import { filter, map, mapTo, takeWhile, startWith, take } from 'rxjs/operators'

import type { ProgressLog, StageLog } from '@pnpm/types'

import { zoomOut } from './utils/zooming.js'
import { hlValue } from './outputConstants.js'

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
    throttle?: Rx.OperatorFunction<any, any> | undefined
    hideAddedPkgsProgress?: boolean | undefined
    hideProgressPrefix?: boolean | undefined
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
    throttle?: Rx.OperatorFunction<any, any> | undefined
    hideAddedPkgsProgress?: boolean | undefined
  },
  importingDone$: Rx.Observable<boolean>,
  progress$: Rx.Observable<ProgressStats>
): Rx.Observable<{
  done: boolean;
  fixed: boolean;
  msg: string;
} | {
  fixed: boolean;
  msg: string;
  done?: never;
}> {
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
    .forEach((log: StageLog): void => {
      const stage = stagePushStreamByRequirer[log.prefix]

      if (typeof stage === 'undefined') {
        const subjectLog = new Rx.Subject<StageLog>()

        stagePushStreamByRequirer[log.prefix] = new Rx.Subject<StageLog>()

        if (!progressStatsPushStreamByRequirer[log.prefix]) {
          progressStatsPushStreamByRequirer[log.prefix] = new Rx.Subject<ProgressStats>()
        }

        const progress = progressStatsPushStreamByRequirer[log.prefix]

        if (typeof progress !== 'undefined') {
          modulesInstallProgressPushStream.next({
            importingDone$: stage$ToImportingDone$(
              Rx.from(subjectLog)
            ),
            progress$: Rx.from(progress),
            requirer: log.prefix,
          })
        }
      }

      stagePushStreamByRequirer[log.prefix]?.next(log)

      if (log.stage === 'importing_done') {
        progressStatsPushStreamByRequirer[log.prefix]?.complete()

        stagePushStreamByRequirer[log.prefix]?.complete()
      }
    })
    .catch((err) => {
      console.error(err)
    })

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
      const prev = previousProgressStatsByRequirer[log.requester]

      if (typeof prev === 'undefined') {
        previousProgressStatsByRequirer[log.requester] = {
          fetched: 0,
          imported: 0,
          resolved: 0,
          reused: 0,
        }
      }

      const current = previousProgressStatsByRequirer[log.requester]

      if (typeof current === 'undefined') {
        return;
      }

      switch (log.status) {
        case 'resolved': {
          current.resolved++

          break
        }

        case 'fetched': {
          current.fetched++

          break
        }

        case 'found_in_store': {
          current.reused++

          break
        }

        case 'imported': {
          current.imported++

          break
        }
      }

      if (!progressStatsPushStreamByRequirer[log.requester]) {
        progressStatsPushStreamByRequirer[log.requester] = new Rx.Subject<ProgressStats>()
      }

      progressStatsPushStreamByRequirer[log.requester]?.next(
        current
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
]): {
  done: true;
  fixed: boolean;
  msg: string;
} | {
  done: false
  fixed: boolean;
  msg: string;
} {
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
    done: false,
  }
}

function createStatusMessageWithoutAdded([progress, importingDone]: [
  ProgressStats,
  boolean,
]): {
  done: true;
  fixed: boolean;
  msg: string;
} | {
  done: false;
  fixed: boolean;
  msg: string;
} {
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
    done: false,
  }
}
