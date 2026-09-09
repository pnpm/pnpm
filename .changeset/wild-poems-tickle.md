---
"@pnpm/fetching.binary-fetcher": patch
"pnpm": patch
---

pnpm now unpacks a downloaded runtime archive into a randomly named directory inside the store. Extraction used to reuse a fixed path, so on a store shared by several users another user could plant a symlink there and redirect a write outside the store.
