/**
 * The pagination control shared by every long list.
 *
 * Long lists are paged rather than scrolled: a day can hold hundreds of blocks,
 * and every row on screen is paint work on every scroll.
 */

import { useTranslation } from '../i18n';

export function Pager({
  offset,
  pageSize,
  total,
  shown,
  onChange,
}: {
  offset: number;
  pageSize: number;
  total: number;
  /** How many rows this page actually holds; the last page is usually short. */
  shown: number;
  onChange: (offset: number) => void;
}) {
  const { t } = useTranslation();
  const last = offset + shown;
  const pageIndex = Math.floor(offset / pageSize) + 1;
  const pageCount = Math.max(1, Math.ceil(total / pageSize));

  return (
    <div className="pagination">
      <span>{t('pager.range', { from: offset + 1, to: last, total })}</span>
      <button disabled={offset === 0} onClick={() => onChange(0)} title={t('pager.firstPage')}>
        «
      </button>
      <button disabled={offset === 0} onClick={() => onChange(Math.max(0, offset - pageSize))}>
        {t('pager.previous')}
      </button>
      <span>{t('pager.page', { page: pageIndex, pages: pageCount })}</span>
      <button disabled={last >= total} onClick={() => onChange(offset + pageSize)}>
        {t('pager.next')}
      </button>
      <button
        disabled={last >= total}
        onClick={() => onChange((pageCount - 1) * pageSize)}
        title={t('pager.lastPage')}
      >
        »
      </button>
    </div>
  );
}
