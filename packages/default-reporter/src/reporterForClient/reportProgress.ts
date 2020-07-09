import { ImportingLog, ProgressLog, StageLog } from '@pnpm/core-loggers'
import PushStream from '@zkochan/zen-push'
import most = require('most')
import { hlValue } from './outputConstants'
import { zoomOut } from './utils/zooming'

type ProgressStats = {
  fetched: number,
  resolved: number,
  reused: number,
}

type ImportingStats = {
  hardLinked: number,
  copied: number,
}

type ModulesInstallProgress = {
  importingDone$: most.Stream<boolean>,
  progress$: most.Stream<ProgressStats>,
  importing$: most.Stream<ImportingStats>,
  requirer: string,
}

export default (
  log$: {
    progress: most.Stream<ProgressLog>,
    importing: most.Stream<ImportingLog>,
    stage: most.Stream<StageLog>,
  },
  opts: {
    cwd: string,
    throttleProgress?: number,
  }
) => {
  const progressOutput = typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0
    ? throttledProgressOutput.bind(null, opts.throttleProgress)
    : nonThrottledProgressOutput

  return getModulesInstallProgress$(log$.stage, log$.progress, log$.importing)
    .map(({ importingDone$, progress$, importing$, requirer }) => {
      const output$ = progressOutput(importingDone$, progress$, importing$)

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
  importing$: most.Stream<ImportingStats>
) {
  // Reporting is done every `throttleProgress` milliseconds
  // and once all packages are fetched.
  const sampler = most.merge(
    most.periodic(throttleProgress).until(importingDone$.skip(1)),
    importingDone$
  )
  return most.sample(
    createStatusMessage,
    sampler,
    progress$,
    importing$,
    importingDone$
  )
  // Avoid logs after all resolved packages were downloaded.
  // Fixing issue: https://github.com/pnpm/pnpm/issues/1028#issuecomment-364782901
  .skipAfter((msg) => msg['done'] === true)
}

function nonThrottledProgressOutput (
  importingDone$: most.Stream<boolean>,
  progress$: most.Stream<ProgressStats>,
  importing$: most.Stream<ImportingStats>
) {
  return most.combine(
    createStatusMessage,
    progress$,
    importing$,
    importingDone$
  )
}

function getModulesInstallProgress$ (
  stage$: most.Stream<StageLog>,
  progress$: most.Stream<ProgressLog>,
  importing$: most.Stream<ImportingLog>
): most.Stream<ModulesInstallProgress> {
  const modulesInstallProgressPushStream = new PushStream<ModulesInstallProgress>()
  const progessStatsPushStreamByRequirer = getProgessStatsPushStreamByRequirer(progress$)
  const importingStatsPushStream = getImportingStatsPushStream(importing$)

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
          importing$: most.from(importingStatsPushStream.observable),
          importingDone$: stage$ToImportingDone$(most.from(stagePushStreamByRequirer[log.prefix].observable)),
          progress$: most.from(progessStatsPushStreamByRequirer[log.prefix].observable),
          requirer: log.prefix,
        })
      }
      setTimeout(() => { // without this, `pnpm m i` might hang in some cases
        stagePushStreamByRequirer[log.prefix].next(log)
        if (log.stage === 'importing_done') {
          progessStatsPushStreamByRequirer[log.prefix].complete()
          stagePushStreamByRequirer[log.prefix].complete()
          importingStatsPushStream.complete()
        }
      }, 0)
    })

  return most.from(modulesInstallProgressPushStream.observable)
}

function stage$ToImportingDone$ (stage$: most.Stream<StageLog>) {
  return stage$
    .filter((log: StageLog) => log.stage === 'importing_done')
    .constant(true)
    .take(1)
    .startWith(false)
    .multicast()
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
        progessStatsPushStreamByRequirer[log.requester] = new PushStream<ProgressStats>()
      }
      progessStatsPushStreamByRequirer[log.requester].next(previousProgressStatsByRequirer[log.requester])
    })

  return progessStatsPushStreamByRequirer
}

function getImportingStatsPushStream (importing$: most.Stream<ImportingLog>) {
  const importingStatsPushStream = new PushStream<ImportingStats>()

  const previousImportingStats: ImportingStats = {
    copied: 0,
    hardLinked: 0,
  }
  importing$
    .forEach((log: ImportingLog) => {
      switch (log.method) {
        case 'hardlink':
          previousImportingStats.hardLinked++
          break
        case 'copy':
          previousImportingStats.copied++
          break
      }
      importingStatsPushStream.next(previousImportingStats)
    })

  return importingStatsPushStream
}

function createStatusMessage (
  progress: ProgressStats,
  importing: ImportingStats,
  importingDone: boolean
) {
  const msg = `Resolving: total ${hlValue(progress.resolved.toString())}, reused ${hlValue(progress.reused.toString())}, downloaded ${hlValue(progress.fetched.toString())}`
  if (importingDone) {
    const importingMsg = `In node_moduels/.pnpm, hardlinked ${hlValue(importing.hardLinked.toString())} packages, copied  ${hlValue(importing.copied.toString())} packages.`
    return {
      done: true,
      fixed: false,
      msg: `${msg}, done\n${importingMsg}`,
    }
  }
  return {
    fixed: true,
    msg,
  }
}
