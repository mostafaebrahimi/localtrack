import { useEffect } from 'react';
import { Navigate, Route, Routes } from 'react-router-dom';

import { AppShell } from '../components/AppShell';
import { I18nProvider, directionOf, resolveLocale } from '../i18n';
import { OnboardingDialog } from '../features/onboarding/OnboardingDialog';
import { useManagedStatus, useSettings } from '../hooks/useLocalTrack';
import { CategoriesRulesPage } from '../pages/CategoriesRulesPage';
import { PrivacyPage } from '../pages/PrivacyPage';
import { ProjectsPage } from '../pages/ProjectsPage';
import { ReportsPage } from '../pages/ReportsPage';
import { SessionsPage } from '../pages/SessionsPage';
import { SettingsPage } from '../pages/SettingsPage';
import { SharedWithWorkPage } from '../pages/SharedWithWorkPage';
import { TimelinePage } from '../pages/TimelinePage';
import { TodayPage } from '../pages/TodayPage';

export function App() {
  const settings = useSettings();
  const managed = useManagedStatus();
  const theme = settings.data?.settings.theme ?? 'system';
  const language = settings.data?.settings.language ?? 'system';

  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = theme;
    if (theme === 'system') {
      const media = window.matchMedia('(prefers-color-scheme: dark)');
      const apply = () => root.classList.toggle('prefers-dark', media.matches);
      apply();
      media.addEventListener('change', apply);
      return () => media.removeEventListener('change', apply);
    }
    root.classList.remove('prefers-dark');
    return undefined;
  }, [theme]);

  // Direction belongs on the document, not on a wrapper: it decides how the
  // scrollbar, text selection and every logical margin in the stylesheet are
  // laid out, and those are the document's own.
  useEffect(() => {
    const locale = resolveLocale(language);
    document.documentElement.lang = locale;
    document.documentElement.dir = directionOf(locale);
  }, [language]);

  // An enrolled device was set up by an administrator; the welcome flow would
  // both be redundant and tell the person something that is no longer true.
  const needsOnboarding =
    settings.data && managed.data
      ? !settings.data.settings.onboardingCompleted && !managed.data.enrolled
      : false;

  return (
    <I18nProvider language={language}>
      <AppShell>
        <Routes>
          <Route path="/" element={<TodayPage />} />
          <Route path="/timeline" element={<TimelinePage />} />
          <Route path="/reports" element={<ReportsPage />} />
          <Route path="/sessions" element={<SessionsPage />} />
          <Route path="/organize" element={<CategoriesRulesPage />} />
          <Route path="/projects" element={<ProjectsPage />} />
          <Route path="/privacy" element={<PrivacyPage />} />
          <Route path="/shared" element={<SharedWithWorkPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </AppShell>
      {needsOnboarding ? <OnboardingDialog /> : null}
    </I18nProvider>
  );
}
