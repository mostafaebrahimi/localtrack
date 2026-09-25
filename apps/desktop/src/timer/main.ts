/**
 * The floating timer bar.
 *
 * Deliberately its own page with no framework: this window is 540×40 pixels and
 * runs all day, so loading the dashboard's React, router, query cache and chart
 * library into a second webview would cost hundreds of megabytes for a clock.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import { makeTranslation } from '../i18n';
import { formatDurationHms } from '../services/format';
import type { CurrentStatus, SettingsView } from '../types';

import './timer.css';

let status: CurrentStatus | null = null;
let busy = false;

/**
 * The bar speaks the same language as the dashboard.
 *
 * It starts in whatever the system suggests and corrects itself once the
 * setting arrives: this window opens with the application, before any command
 * has answered, and a bar that renders nothing until then is worse than one
 * that renders in the wrong language for a frame.
 */
let translation = makeTranslation('system');

const found = document.getElementById('timer');
if (!found) throw new Error('missing #timer element');
const root: HTMLElement = found;

function markup(): string {
  const { t } = translation;
  return `
  <div class="timer-bar" data-tauri-drag-region>
    <span class="timer-grip" data-tauri-drag-region title="${t('timer.drag')}"></span>
    <span class="timer-dot" data-tauri-drag-region></span>
    <span class="timer-time" data-tauri-drag-region>00:00:00</span>
    <span class="timer-state" data-tauri-drag-region hidden></span>
    <span class="timer-activity" data-tauri-drag-region></span>
    <span class="timer-actions">
      <button class="timer-btn start" data-action="clock_in">${icon('play')}${t('timer.start')}</button>
      <button class="timer-btn" data-action="break">${icon('pause')}${t('timer.break')}</button>
      <button class="timer-btn" data-action="resume">${icon('play')}${t('timer.resume')}</button>
      <button class="timer-btn" data-action="clock_out">${icon('stop')}${t('timer.stop')}</button>
      <button class="timer-btn icon" data-action="dashboard" title="${t('timer.openDashboard')}">${icon('expand')}</button>
      <button class="timer-btn icon" data-action="hide" title="${t('timer.hide')}">${icon('close')}</button>
    </span>
  </div>
`;
}

/** Rebuilt rather than patched: six labels change together, once, rarely. */
let el = mount();

function mount() {
  root.innerHTML = markup();
  document.documentElement.lang = translation.locale;
  document.documentElement.dir = translation.dir;
  return {
    bar: root.querySelector('.timer-bar') as HTMLElement,
    dot: root.querySelector('.timer-dot') as HTMLElement,
    time: root.querySelector('.timer-time') as HTMLElement,
    state: root.querySelector('.timer-state') as HTMLElement,
    activity: root.querySelector('.timer-activity') as HTMLElement,
  };
}

function icon(name: 'play' | 'pause' | 'stop' | 'expand' | 'close'): string {
  const open = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">';
  const shapes = {
    play: '<path d="M7 4.5 19 12 7 19.5z" fill="currentColor" stroke="none"/>',
    pause:
      '<rect x="6.5" y="4.5" width="4" height="15" rx="1.2" fill="currentColor" stroke="none"/><rect x="13.5" y="4.5" width="4" height="15" rx="1.2" fill="currentColor" stroke="none"/>',
    stop: '<rect x="5.5" y="5.5" width="13" height="13" rx="2" fill="currentColor" stroke="none"/>',
    expand: '<path d="M14 4h6v6M20 4l-7.5 7.5M10 20H4v-6M4 20l7.5-7.5"/>',
    close: '<path d="M6 6l12 12M18 6L6 18"/>',
  };
  return `${open}${shapes[name]}</svg>`;
}

/** Elapsed time ticks locally; the backend only has to send state changes. */
function elapsedMs(): number {
  if (!status) return 0;
  if (status.state === 'ON_BREAK' && status.openBreak) {
    return Date.now() - status.openBreak.startedAtMs;
  }
  if (status.session) return Date.now() - status.session.startedAtMs;
  return 0;
}

function render(): void {
  const state = status?.state ?? 'CLOCKED_OUT';
  const paused = status?.tracking === 'PAUSED';
  const tone =
    paused ? 'paused' : state === 'CLOCKED_IN' ? 'on' : state === 'ON_BREAK' ? 'break' : 'off';

  el.bar.className = `timer-bar tone-${tone}${state === 'CLOCKED_OUT' ? ' nudge' : ''}`;
  el.dot.className = `timer-dot ${tone}`;
  el.time.textContent = formatDurationHms(elapsedMs());

  const { t } = translation;
  const word =
    state === 'CLOCKED_OUT'
      ? t('timer.notTracking')
      : paused
        ? t('timer.paused')
        : state === 'ON_BREAK'
          ? t('timer.onBreak')
          : '';
  el.state.textContent = word;
  el.state.hidden = word.length === 0;

  const message =
    state === 'CLOCKED_OUT'
      ? t('timer.notRunning')
      : paused
        ? t('timer.nothingRecorded')
        : (status?.currentActivity?.label ?? t('timer.waiting'));
  el.activity.textContent = message;
  el.activity.title = message;

  for (const button of root.querySelectorAll<HTMLButtonElement>('.timer-btn')) {
    const action = button.dataset.action ?? '';
    const visible =
      action === 'clock_in'
        ? state === 'CLOCKED_OUT'
        : action === 'break'
          ? state === 'CLOCKED_IN'
          : action === 'resume'
            ? state === 'ON_BREAK'
            : action === 'clock_out'
              ? state !== 'CLOCKED_OUT'
              : true;
    button.hidden = !visible;
    button.disabled = busy;
  }
}

async function act(action: string): Promise<void> {
  if (busy) return;
  busy = true;
  render();
  try {
    switch (action) {
      case 'clock_in':
        status = await invoke<CurrentStatus>('clock_in', { note: null });
        break;
      case 'break':
        status = await invoke<CurrentStatus>('start_break');
        break;
      case 'resume':
        status = await invoke<CurrentStatus>('end_break');
        break;
      case 'clock_out':
        status = await invoke<CurrentStatus>('clock_out');
        break;
      case 'dashboard':
        await invoke('open_dashboard');
        break;
      case 'hide':
        // Hiding is a setting: the desktop process keeps the window in step
        // with it, so hiding the window alone would be undone a second later.
        await invoke('update_setting', { key: 'show_mini_timer', value: false });
        break;
    }
  } catch (error) {
    console.warn('[LocalTrack] action failed', error);
  } finally {
    busy = false;
    render();
  }
}

root.addEventListener('click', (event) => {
  const button = (event.target as HTMLElement).closest<HTMLButtonElement>('.timer-btn');
  if (button?.dataset.action) void act(button.dataset.action);
});

void listen<CurrentStatus>('localtrack://status', (event) => {
  status = event.payload;
  render();
});

// The language is a setting, so it has to be asked for; until it answers the
// bar shows the system's own language.
void invoke<SettingsView>('get_settings')
  .then((view) => {
    translation = makeTranslation(view.settings.language);
    el = mount();
    render();
  })
  .catch(() => undefined);

void invoke<CurrentStatus>('get_current_status')
  .then((value) => {
    status = value;
    render();
  })
  .catch(() => render());

// The clock ticks here rather than being pushed from the backend.
setInterval(render, 1000);
render();
