import {EventEmitter} from 'events'
import logUpdate = require('log-update')
import R = require('ramda')
import {Log} from 'supi'
import xs, {Stream} from 'xstream'
import fromEvent from 'xstream/extra/fromEvent'
import mergeOutputs from './mergeOutputs'
import reporterForClient from './reporterForClient'

export default function (
  streamParser: object,
  cmd?: string, // is optional only to be backward compatible
) {
  toOutput$(streamParser, cmd)
    .subscribe({
      complete () {}, // tslint:disable-line:no-empty
      error: (err) => logUpdate(err.message),
      next: logUpdate,
    })
}

export function toOutput$ (
  streamParser: object,
  cmd?: string, // is optional only to be backward compatible
): Stream<string> {
  const isRecursive = cmd === 'recursive'
  const obs = fromEvent(streamParser as EventEmitter, 'data')
  const log$ = xs.fromObservable<Log>(obs)
  const outputs: Array<xs<xs<{msg: string}>>> = reporterForClient(log$, isRecursive)

  return mergeOutputs(outputs)
}
