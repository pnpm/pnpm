export function createCancellationCheck (control) {
  const state = control && new Int32Array(control)
  return () => {
    if (state && Atomics.load(state, 0)) throw new WebAssembly.RuntimeError('WASM worker cancelled')
  }
}
