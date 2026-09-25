import type { ReactNode } from 'react';
import { NavLink } from 'react-router-dom';

import { useTranslation, type MessageKey } from '../i18n';
import { useManagedStatus, useStatusSelector } from '../hooks/useLocalTrack';
import { useAppStore } from '../stores/useAppStore';
import { Toast } from './Toast';

interface NavItem {
  to: string;
  label: MessageKey;
  end?: boolean;
  /// Hidden in employee mode, where the organization owns this configuration.
  personalOnly?: boolean;
}

const NAV: NavItem[] = [
  { to: '/', label: 'nav.today', end: true },
  { to: '/timeline', label: 'nav.timeline' },
  { to: '/reports', label: 'nav.reports' },
  { to: '/sessions', label: 'nav.sessions' },
  { to: '/organize', label: 'nav.organize', personalOnly: true },
  { to: '/projects', label: 'nav.projects' },
  { to: '/privacy', label: 'nav.privacy' },
  { to: '/shared', label: 'nav.shared' },
  { to: '/settings', label: 'nav.settings' },
];

export function AppShell({ children }: { children: ReactNode }) {
  // The footer needs three values that almost never change; selecting them
  // keeps the shell out of the per-second status churn.
  const footer = useStatusSelector((status) => ({
    chromeConnected: status.chromeConnected,
    appVersion: status.appVersion,
    schemaVersion: status.schemaVersion,
  }));
  const managed = useManagedStatus();
  const toast = useAppStore((state) => state.toast);
  const { t } = useTranslation();

  const employee = managed.data?.mode === 'EMPLOYEE';
  const items = NAV.filter((item) => !(employee && item.personalOnly));

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          LocalTrack
          <small>
            {employee
              ? t('brand.managedBy', {
                  organization: managed.data?.organization ?? t('managed.yourOrganization'),
                })
              : t('brand.tagline')}
          </small>
        </div>
        <nav>
          {items.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={({ isActive }) => (isActive ? 'active' : '')}
            >
              {t(item.label)}
            </NavLink>
          ))}
        </nav>
        <div className="spacer" />
        <div className="footer">
          <div>
            {footer.data?.chromeConnected ? (
              <span className="badge ok">{t('shell.chromeConnected')}</span>
            ) : (
              <span className="badge warn">{t('shell.chromeNotConnected')}</span>
            )}
          </div>
          <div style={{ marginTop: 8 }}>
            LocalTrack {footer.data?.appVersion ?? ''}
            <br />
            {t('shell.schema', { version: footer.data?.schemaVersion ?? '' })}
          </div>
        </div>
      </aside>
      <main className="main">
        {employee ? (
          <div className="managed-banner">
            <span className="managed-dot" />
            <span>
              {t('managed.banner', {
                organization: managed.data?.organization ?? t('managed.yourOrganization'),
                host: managed.data?.serverHost ?? '',
              })}
            </span>
            <NavLink to="/shared">{t('managed.seeShared')}</NavLink>
          </div>
        ) : null}
        {children}
      </main>
      {toast ? <Toast /> : null}
    </div>
  );
}
