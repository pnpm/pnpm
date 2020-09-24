export type ResolveFunction<T> = (value?: T | PromiseLike<T>) => void

export type RejectFunction = (reason?: Error) => void

export interface Deferred<T> {
  resolve: ResolveFunction<T>
  reject: RejectFunction
  promise: Promise<T>
}

export default function <T> (): Deferred<T> {
  let resolve_: ResolveFunction<T>
  let reject_: RejectFunction
  // Disable empty function errors.
  resolve_ = (value) => {} // eslint-disable-line
  reject_ = (reason) => {} // eslint-disable-line
  const promise = new Promise<T>((resolve, reject) => {
    resolve_ = resolve
    reject_ = reject
  })
  return {
    promise,
    reject: reject_,
    resolve: resolve_,
  }
}
