// WebContainers cannot interrupt a worker parked in an indefinite Atomics.wait.
export function waitForReply (state, index, checkCancelled) {
  while (Atomics.load(state, index) === 0) {
    checkCancelled?.()
    Atomics.wait(state, index, 0, 100)
  }
}
