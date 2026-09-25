import { useState } from 'react';

import { ReportTable } from '../components/ReportTable';
import { DateRangeBar } from '../components/DateRangeBar';
import { ErrorState, Loading } from '../components/States';
import { useTranslation } from '../i18n';
import { api } from '../services/api';
import {
  queryKeys,
  useAppMutation,
  useProjects,
  useRangeQuery,
  useReports,
} from '../hooks/useLocalTrack';

export function ProjectsPage() {
  const { t } = useTranslation();
  const projects = useProjects(true);
  const query = useRangeQuery();
  const reports = useReports(query);
  const [name, setName] = useState('');

  const invalidate = [queryKeys.projects, ['reports'], queryKeys.today];
  const create = useAppMutation((value: string) => api.createProject(value), invalidate, 'toast.projectCreated');
  const update = useAppMutation(
    (payload: { id: string; name: string; archived: boolean }) =>
      api.updateProject(payload.id, payload.name, payload.archived),
    invalidate,
  );
  const remove = useAppMutation((id: string) => api.deleteProject(id), invalidate, 'toast.projectDeleted');

  if (projects.isLoading) return <Loading />;
  if (projects.error) {
    return <ErrorState error={projects.error} onRetry={() => void projects.refetch()} />;
  }

  return (
    <div>
      <div className="page-header">
        <div>
          <h1>{t('projects.title')}</h1>
          <p>{t('projects.subtitle')}</p>
        </div>
        <DateRangeBar />
      </div>

      <div className="grid-2">
        <section className="card">
          <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('projects.yours')}</h2>
          <table>
            <thead>
              <tr>
                <th>{t('table.name')}</th>
                <th>{t('projects.archived')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {projects.data?.map((project) => (
                <tr key={project.id}>
                  <td>
                    <input
                      defaultValue={project.name}
                      onBlur={(event) => {
                        if (event.target.value && event.target.value !== project.name) {
                          update.mutate({
                            id: project.id,
                            name: event.target.value,
                            archived: project.archived,
                          });
                        }
                      }}
                    />
                  </td>
                  <td>
                    <input
                      type="checkbox"
                      checked={project.archived}
                      onChange={(event) =>
                        update.mutate({
                          id: project.id,
                          name: project.name,
                          archived: event.target.checked,
                        })
                      }
                    />
                  </td>
                  <td className="numeric">
                    <button className="ghost" onClick={() => remove.mutate(project.id)}>
                      {t('common.delete')}
                    </button>
                  </td>
                </tr>
              ))}
              {projects.data?.length === 0 ? (
                <tr>
                  <td colSpan={3} className="muted small">
                    {t('projects.none')}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
          <div className="row" style={{ marginTop: 10 }}>
            <input
              placeholder={t('projects.newPlaceholder')}
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
            <button
              onClick={() => {
                if (name.trim()) {
                  create.mutate(name.trim());
                  setName('');
                }
              }}
            >
              {t('projects.add')}
            </button>
          </div>
        </section>

        <ReportTable report={reports.data?.projects} title={t('projects.timeByProject')} />
      </div>
    </div>
  );
}
