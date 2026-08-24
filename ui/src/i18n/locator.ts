import { t } from './index';
import type { Coordinate } from '../lib/ipc';
export type { Coordinate };

const range = (a: number, b: number) => (a === b ? `${a}` : `${a}–${b}`);

export function formatLocator(c: Coordinate): string {
  switch (c.kind) {
    case 'page': return `${t('loc_page')} ${c.number}`;
    case 'line': return `${t(c.start === c.end ? 'loc_line_one' : 'loc_line_many')} ${range(c.start, c.end)}`;
    case 'sheet_rows': return `${t('loc_sheet')} ${c.sheet}, ${t(c.start === c.end ? 'loc_row_one' : 'loc_row_many')} ${range(c.start, c.end)}`;
    case 'section': return c.title;
    case 'none': return '';
  }
}
