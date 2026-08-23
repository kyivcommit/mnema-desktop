import { t } from './index';
import type { RefusalKind } from '../lib/ipc';
export type { RefusalKind };

export function refusalText(kind: RefusalKind): string {
  switch (kind) {
    case 'noCandidates': return t('refusal_no_candidates');
    case 'emptyCompletion': return t('refusal_empty_completion');
  }
}
