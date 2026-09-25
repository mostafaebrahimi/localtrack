import { useTranslation } from '../i18n';
import { useCategories, useFilterOptions, useProjects } from '../hooks/useLocalTrack';
import { useAppStore } from '../stores/useAppStore';

/** Global filters (spec §81). */
export function FilterBar() {
  const { t } = useTranslation();
  const filter = useAppStore((state) => state.filter);
  const patch = useAppStore((state) => state.patchFilter);
  const clear = useAppStore((state) => state.clearFilter);
  const options = useFilterOptions();
  const categories = useCategories();
  const projects = useProjects();

  const single = (values: string[] | undefined) => values?.[0] ?? '';

  return (
    <div className="card" style={{ marginBottom: 16 }}>
      <div className="row">
        <input
          type="search"
          placeholder={t('filter.searchPlaceholder')}
          style={{ minWidth: 260 }}
          value={filter.search ?? ''}
          onChange={(event) => patch({ search: event.target.value || null })}
        />
        <select
          value={single(filter.appNames)}
          onChange={(event) =>
            patch({ appNames: event.target.value ? [event.target.value] : [] })
          }
          aria-label={t('filter.application')}
        >
          <option value="">{t('filter.allApplications')}</option>
          {options.data?.applications.map((app) => (
            <option key={app} value={app}>
              {app}
            </option>
          ))}
        </select>
        <select
          value={single(filter.domains)}
          onChange={(event) => patch({ domains: event.target.value ? [event.target.value] : [] })}
          aria-label={t('filter.domain')}
        >
          <option value="">{t('filter.allWebsites')}</option>
          {options.data?.domains.map((domain) => (
            <option key={domain} value={domain}>
              {domain}
            </option>
          ))}
        </select>
        <select
          value={single(filter.categoryIds)}
          onChange={(event) =>
            patch({ categoryIds: event.target.value ? [event.target.value] : [] })
          }
          aria-label={t('filter.category')}
        >
          <option value="">{t('filter.allCategories')}</option>
          {categories.data?.map((category) => (
            <option key={category.id} value={category.id}>
              {category.name}
            </option>
          ))}
        </select>
        <select
          value={single(filter.projectIds)}
          onChange={(event) =>
            patch({ projectIds: event.target.value ? [event.target.value] : [] })
          }
          aria-label={t('filter.project')}
        >
          <option value="">{t('filter.allProjects')}</option>
          {projects.data?.map((project) => (
            <option key={project.id} value={project.id}>
              {project.name}
            </option>
          ))}
        </select>
        <select
          value={filter.kinds?.[0] ?? ''}
          onChange={(event) => patch({ kinds: event.target.value ? [event.target.value] : [] })}
          aria-label={t('filter.activityKind')}
        >
          <option value="">{t('filter.allActivity')}</option>
          <option value="WINDOW">{t('filter.applications')}</option>
          <option value="BROWSER_PAGE">{t('filter.websites')}</option>
          <option value="IDLE">{t('filter.idle')}</option>
          <option value="LOCKED">{t('filter.locked')}</option>
        </select>
        <select
          value={filter.minDurationMs ? String(filter.minDurationMs) : ''}
          onChange={(event) =>
            patch({ minDurationMs: event.target.value ? Number(event.target.value) : null })
          }
          aria-label={t('filter.minDuration')}
        >
          <option value="">{t('filter.anyDuration')}</option>
          <option value="30000">{t('filter.from30s')}</option>
          <option value="300000">{t('filter.from5m')}</option>
          <option value="1800000">{t('filter.from30m')}</option>
        </select>
        <button className="ghost" onClick={clear}>
          {t('filter.clear')}
        </button>
      </div>
    </div>
  );
}
