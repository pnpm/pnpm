import pShare = require('promise-share')

export default function safeDeferredPromise<T> () {
  let resolve!: (v: T) => void
  let reject!: (err: Error) => void

  const promiseFn = pShare(new Promise<T>((_resolve, _reject) => {
    resolve = _resolve
    reject = _reject
  }))

  return Object.assign(promiseFn, { resolve, reject })
}
