import { useState } from 'react';

import { ErrorState, Loading } from '../components/States';
import { api } from '../services/api';
import {
  queryKeys,
  useAppMutation,
  useCategories,
  useProjects,
  useRules,
} from '../hooks/useLocalTrack';
import { useTranslation, type MessageKey } from '../i18n';
import type { ClassificationRule, RuleField, RuleOperator } from '../types';

const FIELDS: { id: RuleField; label: MessageKey }[] = [
  // The workspace comes first: a rule about the work reads better than a rule
  // about the application it happened in.
  { id: 'workspace', label: 'field.workspace' },
  { id: 'app_name', label: 'field.app_name' },
  { id: 'process_name', label: 'field.process_name' },
  { id: 'window_title', label: 'field.window_title' },
  { id: 'browser', label: 'field.browser' },
  { id: 'domain', label: 'field.domain' },
  { id: 'url', label: 'field.url' },
  { id: 'page_title', label: 'field.page_title' },
];

const OPERATORS: RuleOperator[] = [
  'EXACT',
  'CONTAINS',
  'STARTS_WITH',
  'ENDS_WITH',
  'GLOB',
  'REGEX',
];

const emptyRule: ClassificationRule = {
  id: '',
  name: '',
  enabled: true,
  priority: 10,
  targetField: 'domain',
  operator: 'CONTAINS',
  pattern: '',
  categoryId: null,
  projectId: null,
  createdAtMs: 0,
  updatedAtMs: 0,
};

export function CategoriesRulesPage() {
  const { t } = useTranslation();
  const categories = useCategories();
  const projects = useProjects();
  const rules = useRules();
  const [draft, setDraft] = useState<ClassificationRule | null>(null);
  const [applyToExisting, setApplyToExisting] = useState(false);
  const [newCategory, setNewCategory] = useState('');

  const invalidate = [queryKeys.rules, queryKeys.categories, queryKeys.today, ['reports']];

  const saveRule = useAppMutation(
    (rule: ClassificationRule) => api.saveRule(rule, applyToExisting),
    invalidate,
    'toast.ruleSaved',
  );
  const deleteRule = useAppMutation(
    (id: string) => api.deleteRule(id, applyToExisting),
    invalidate,
    'toast.ruleDeleted',
  );
  const createCategory = useAppMutation(
    (name: string) => api.createCategory(name),
    invalidate,
    'toast.categoryAdded',
  );
  const renameCategory = useAppMutation(
    (payload: { id: string; name: string }) => api.renameCategory(payload.id, payload.name),
    invalidate,
  );
  const deleteCategory = useAppMutation(
    (id: string) => api.deleteCategory(id),
    invalidate,
    'toast.categoryDeleted',
  );
  const reclassify = useAppMutation(
    () => api.reclassify(null, null),
    invalidate,
    'toast.reclassified',
  );

  if (rules.isLoading || categories.isLoading) return <Loading />;
  if (rules.error) return <ErrorState error={rules.error} onRetry={() => void rules.refetch()} />;

  return (
    <div>
      <div className="page-header">
        <div>
          <h1>{t('organize.title')}</h1>
          <p>{t('organize.subtitle')}</p>
        </div>
        <button onClick={() => setDraft({ ...emptyRule })}>{t('organize.newRule')}</button>
      </div>

      <div className="grid-2">
        <section className="card">
          <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('organize.categories')}</h2>
          <table>
            <tbody>
              {categories.data?.map((category) => (
                <tr key={category.id}>
                  <td>
                    <input
                      defaultValue={category.name}
                      onBlur={(event) => {
                        if (event.target.value && event.target.value !== category.name) {
                          renameCategory.mutate({ id: category.id, name: event.target.value });
                        }
                      }}
                    />
                  </td>
                  <td className="numeric">
                    <button className="ghost" onClick={() => deleteCategory.mutate(category.id)}>
                      {t('common.delete')}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="row" style={{ marginTop: 10 }}>
            <input
              placeholder={t('organize.newCategory')}
              value={newCategory}
              onChange={(event) => setNewCategory(event.target.value)}
            />
            <button
              onClick={() => {
                if (newCategory.trim()) {
                  createCategory.mutate(newCategory.trim());
                  setNewCategory('');
                }
              }}
            >
              {t('common.add')}
            </button>
          </div>
        </section>

        <section className="card">
          <div className="row between">
            <h2 style={{ fontSize: 15, marginTop: 0 }}>{t('organize.rules')}</h2>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={applyToExisting}
                onChange={(event) => setApplyToExisting(event.target.checked)}
              />
              {t('organize.applyChanges')}
            </label>
          </div>
          <table>
            <thead>
              <tr>
                <th>{t('table.name')}</th>
                <th>{t('organize.match')}</th>
                <th className="numeric">{t('organize.priority')}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rules.data?.map((rule) => (
                <tr key={rule.id}>
                  <td>
                    {rule.name}
                    {!rule.enabled ? <span className="badge"> disabled</span> : null}
                  </td>
                  <td className="small muted">
                    {t(`field.${rule.targetField}` as MessageKey)} {t(`op.${rule.operator}` as MessageKey)}{' '}
                    <span className="mono">{rule.pattern}</span>
                  </td>
                  <td className="numeric">{rule.priority}</td>
                  <td className="numeric">
                    <button className="ghost" onClick={() => setDraft(rule)}>
                      {t('common.edit')}
                    </button>
                    <button className="ghost" onClick={() => deleteRule.mutate(rule.id)}>
                      {t('common.delete')}
                    </button>
                  </td>
                </tr>
              ))}
              {rules.data?.length === 0 ? (
                <tr>
                  <td colSpan={4} className="muted small">
                    {t('organize.noRules')}
                  </td>
                </tr>
              ) : null}
            </tbody>
          </table>
          <div className="row" style={{ marginTop: 10 }}>
            <button onClick={() => reclassify.mutate(undefined)}>
              {t('organize.reapply')}
            </button>
          </div>
        </section>
      </div>

      {draft ? (
        <div className="modal-backdrop" role="dialog" aria-modal="true">
          <div className="modal">
            <h2 style={{ marginTop: 0, fontSize: 17 }}>{draft.id ? t('organize.editRule') : t('organize.newRule')}</h2>
            <div className="field">
              <label>{t('table.name')}</label>
              <input
                value={draft.name}
                onChange={(event) => setDraft({ ...draft, name: event.target.value })}
              />
            </div>
            <div className="row">
              <div className="field" style={{ flex: 1 }}>
                <label>{t('organize.field')}</label>
                <select
                  value={draft.targetField}
                  onChange={(event) =>
                    setDraft({ ...draft, targetField: event.target.value as RuleField })
                  }
                >
                  {FIELDS.map((field) => (
                    <option key={field.id} value={field.id}>
                      {t(field.label)}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field" style={{ flex: 1 }}>
                <label>{t('organize.operator')}</label>
                <select
                  value={draft.operator}
                  onChange={(event) =>
                    setDraft({ ...draft, operator: event.target.value as RuleOperator })
                  }
                >
                  {OPERATORS.map((operator) => (
                    <option key={operator} value={operator}>
                      {t(`op.${operator}` as MessageKey)}
                    </option>
                  ))}
                </select>
              </div>
            </div>
            <div className="field">
              <label>{t('organize.pattern')}</label>
              <input
                value={draft.pattern}
                onChange={(event) => setDraft({ ...draft, pattern: event.target.value })}
                placeholder={t('organize.patternPlaceholder')}
              />
            </div>
            <div className="row">
              <div className="field" style={{ flex: 1 }}>
                <label>{t('filter.category')}</label>
                <select
                  value={draft.categoryId ?? ''}
                  onChange={(event) =>
                    setDraft({ ...draft, categoryId: event.target.value || null })
                  }
                >
                  <option value="">{t('organize.noCategory')}</option>
                  {categories.data?.map((category) => (
                    <option key={category.id} value={category.id}>
                      {category.name}
                    </option>
                  ))}
                </select>
              </div>
              <div className="field" style={{ flex: 1 }}>
                <label>{t('filter.project')}</label>
                <select
                  value={draft.projectId ?? ''}
                  onChange={(event) =>
                    setDraft({ ...draft, projectId: event.target.value || null })
                  }
                >
                  <option value="">{t('organize.noProject')}</option>
                  {projects.data
                    ?.filter((project) => !project.archived)
                    .map((project) => (
                      <option key={project.id} value={project.id}>
                        {project.name}
                      </option>
                    ))}
                </select>
              </div>
              <div className="field" style={{ width: 110 }}>
                <label>{t('organize.priority')}</label>
                <input
                  type="number"
                  value={draft.priority}
                  onChange={(event) =>
                    setDraft({ ...draft, priority: Number(event.target.value) || 0 })
                  }
                />
              </div>
            </div>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={draft.enabled}
                onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })}
              />
              {t('organize.enabled')}
            </label>
            <label className="checkbox">
              <input
                type="checkbox"
                checked={applyToExisting}
                onChange={(event) => setApplyToExisting(event.target.checked)}
              />
              {t('organize.applyToExisting')}
            </label>

            <div className="row" style={{ justifyContent: 'flex-end', marginTop: 16 }}>
              <button onClick={() => setDraft(null)}>{t('common.cancel')}</button>
              <button
                className="primary"
                onClick={() => {
                  saveRule.mutate(draft);
                  setDraft(null);
                }}
              >
                {t('organize.saveRule')}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
