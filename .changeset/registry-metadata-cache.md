---
"@pnpm/resolving.npm-resolver": minor
"pnpm": minor
---

Added binary registry metadata cache using SQLite + MessagePack.

Eliminates JSON parsing overhead during dependency resolution by caching parsed
package metadata in a binary format (MessagePack) using SQLite as the backend.
Provides multi-registry isolation and HTTP header caching for efficient updates.
