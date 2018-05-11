export type ResolveFunction<T> =  (value?: T | PromiseLike<T>) => void

export type RejectFunction = (reason?: Error) => void

export interface Deferred<T> {
  resolve: ResolveFunction<T>;
  reject: RejectFunction;
  promise: Promise<T>;
};

export default function <T>(): Deferred<T> {
  let resolve: ResolveFunction<T>
  let reject: RejectFunction
  // Disable empty function errors.
  resolve = (value) => {} // tslint:disable-line
  reject = (reason) => {} // tslint:disable-line
  const promise = new Promise<T>((resolveInner, rejectInner) => {
    resolve = resolveInner
    reject = rejectInner
  })
  return {
    promise,
    reject,
    resolve,
  }
}