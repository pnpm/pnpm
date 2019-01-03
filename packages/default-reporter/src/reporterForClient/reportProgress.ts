import { ProgressLog, StageLog } from '@pnpm/core-loggers'
import PushStream from '@zkochan/zen-push'
import most = require('most')
import { hlValue } from './outputConstants'
import { zoomOut } from './utils/zooming'

type GroupedProgressMessage = {
  modulesDir: string,
  progress$: most.Stream<{
    fetched: number,
    resolved: number,
    reused: number,
  }>,
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
  const groupedProgressPushStream = new PushStream<GroupedProgressMessage>()

  const progessPushStreamByModulesDir: {
    [modulesDir: string]: PushStream<{
      fetched: number,
      resolved: number,
      reused: number,
    }>,
  } = {}

  const stagePushStreamByModulesDir: {
    [modulesDir: string]: PushStream<StageLog>,
  } = {}
  const reportingPushStream = new PushStream()

  most.from(groupedProgressPushStream.observable)
    .forEach((groupedProgress: GroupedProgressMessage) => {
      const importingDone$ = groupedProgress.stage$
        .filter((log: StageLog) => log.stage === 'importing_done')
        .constant(true)
        .take(1)
        .startWith(false)
        .multicast()

      let progress$
      if (typeof opts.throttleProgress === 'number' && opts.throttleProgress > 0) {
        // Reporting is done every `throttleProgress` milliseconds
        // and once all packages are fetched.
        const sampler = most.merge(
          most.periodic(opts.throttleProgress).until(importingDone$),
          importingDone$,
        )
        progress$ = most.sample(
          createStatusMessage,
          sampler,
          groupedProgress.progress$,
          importingDone$,
        )
        // Avoid logs after all resolved packages were downloaded.
        // Fixing issue: https://github.com/pnpm/pnpm/issues/1028#issuecomment-364782901
        .skipAfter((msg) => msg['done'] === true)
      } else {
        progress$ = most.combine(
          createStatusMessage,
          groupedProgress.progress$,
          importingDone$,
        )
      }
      if (groupedProgress.modulesDir === opts.cwd) {
        reportingPushStream.next(progress$)
      } else {
        reportingPushStream.next(progress$.map((msg) => {
          msg['msg'] = zoomOut(opts.cwd, groupedProgress.modulesDir, msg['msg'])
          return msg
        }))
      }
    })

  log$.stage
    .forEach((log: StageLog) => {
      if (!stagePushStreamByModulesDir[log.prefix]) {
        stagePushStreamByModulesDir[log.prefix] = new PushStream<StageLog>()
        if (!progessPushStreamByModulesDir[log.prefix]) {
          progessPushStreamByModulesDir[log.prefix] = new PushStream()
        }
        groupedProgressPushStream.next({
          modulesDir: log.prefix,
          progress$: most.from(progessPushStreamByModulesDir[log.prefix].observable),
          stage$: most.from(stagePushStreamByModulesDir[log.prefix].observable),
        })
      }
      stagePushStreamByModulesDir[log.prefix].next(log)
      if (log.stage === 'importing_done') {
        progessPushStreamByModulesDir[log.prefix].complete()
        stagePushStreamByModulesDir[log.prefix].complete()
      }
    })

  const prevProgressByModulesDir: {
    [modulesDir: string]: {
      fetched: number,
      resolved: number,
      reused: number,
    },
  } = {}
  log$.progress
    .filter((log: ProgressLog) => !!log['context'])
    .forEach((log: ProgressLog) => {
      if (!prevProgressByModulesDir[log['context']]) {
        prevProgressByModulesDir[log['context']] = {
          fetched: 0,
          resolved: 0,
          reused: 0,
        }
      } else {
        prevProgressByModulesDir[log['context']] = {
          ...prevProgressByModulesDir[log['context']]
        }
      }
      switch (log.status) {
        case 'resolving_content':
          prevProgressByModulesDir[log['context']].resolved++
          break
        case 'fetched':
          prevProgressByModulesDir[log['context']].fetched++
          break
        case 'found_in_store':
          prevProgressByModulesDir[log['context']].reused++
          break
      }
      progessPushStreamByModulesDir[log['context']] = progessPushStreamByModulesDir[log['context']] || new PushStream<ProgressLog>()
      progessPushStreamByModulesDir[log['context']].next(prevProgressByModulesDir[log['context']])
    })

  return most.from(reportingPushStream.observable) as most.Stream<most.Stream<{ msg: string }>>

  function createStatusMessage (
    progress: {
      fetched: number,
      resolved: number,
      reused: number,
    },
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
}
