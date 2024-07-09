Performs search across all packages in pnpm workspace.
If there are more than 2 packages with same dependency - it will move dependency to catalog.
If there are multiple versions of dependency - codemod will prompt to choose whether to move to catalog or not.
