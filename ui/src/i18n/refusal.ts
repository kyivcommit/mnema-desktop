import { t } from './index';

export type RefusalKind = 'noCandidates' | 'emptyCompletion';

export function refusalText(kind: RefusalKind): string {
  switch (kind) {
    case 'noCandidates': return t('refusal_no_candidates');
    case 'emptyCompletion': return t('refusal_empty_completion');
  }
}
