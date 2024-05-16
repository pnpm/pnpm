let nodeIdCounter = 0

export function nextNodeId (): string {
  return (++nodeIdCounter).toString()
}
