import { type Modules } from '@pnpm/installing.modules-yaml';
import type { IgnoredBuildsCommandOpts } from './ignoredBuilds.js';
export interface GetAutomaticallyIgnoredBuildsResult {
    automaticallyIgnoredBuilds: string[] | null;
    modulesDir: string;
    modulesManifest: Modules | null;
}
export declare function getAutomaticallyIgnoredBuilds(opts: IgnoredBuildsCommandOpts): Promise<GetAutomaticallyIgnoredBuildsResult>;
