---
"pacquet": patch
---

Added `has_circular_peers` function to detect circular peer dependency chains in lockfiles using a DFS-based approach with visited set and recursion stack. This enables detection of circular peer dependency cycles that could cause infinite walks or incorrect dependency resolution.

The function computes Strongly Connected Components (SCC) using a simplified Tarjan's algorithm, providing package name to component label mapping for analyzing peer dependency graph structure.
