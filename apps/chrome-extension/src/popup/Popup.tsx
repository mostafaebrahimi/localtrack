import { useCallback, useEffect, useState } from 'react';

import type { ClockCommand, StatusResponse } from '@localtrack/protocol';

import type { OperationalState } from '../utils/state';

interface PopupData {
  connection: 'connected' | 'connecting' | 'disconnected';
  state: OperationalState;
  status: StatusResponse | null;
  queued: number;
  dropped: number;
}

function formatDuration(ms: number): string {
  const total = Math.floor(Math.max(ms, 0) / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  return [hours, minutes, seconds].map((value) => String(value).padStart(2, '0')).join(':');
}

function formatShort(ms: number): string {
  const minutes = Math.floor(Math.max(ms, 0) / 60000);
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

export function Popup() {
  const [data, setData] = useState<PopupData | null>(null);
  const [tick, setTick] = useState(0);

  const refresh = useCallback(() => {
    chrome.runtime.sendMessage({ type: 'localtrack:getStatus' }, (response: PopupData) => {
      if (response) setData(response);
    });
  }, []);

  useEffect(() => {
    refresh();
    const timer = setInterval(() => {
      setTick((value) => value + 1);
      refresh();
    }, 1000);
    return () => clearInterval(timer);
  }, [refresh]);

  const sendCommand = (command: ClockCommand) => {
    chrome.runtime.sendMessage({ type: 'localtrack:clock', command }, () => {
      setTimeout(refresh, 250);
    });
  };

  if (!data) {
    return (
      <div className="popup">
        <div className="title">LocalTrack</div>
        <div className="muted">Loading…</div>
      </div>
    );
  }

  const connected = data.connection === 'connected';
  const status = data.status;
  const elapsed =
    status && status.sessionStartedAtMs
      ? Date.now() - status.sessionStartedAtMs
      : (status?.sessionDurationMs ?? 0);

  // The tick keeps the elapsed clock ticking between refreshes.
  void tick;

  return (
    <div className="popup">
      <div className="title">LocalTrack</div>

      <div className="status">
        <span className={`dot ${connected ? 'ok' : 'err'}`} />
        {connected ? 'Connected' : data.connection === 'connecting' ? 'Connecting…' : 'Not connected'}
      </div>

      {!connected ? (
        <>
          <div className="warning">
            Desktop connection unavailable. Install or start LocalTrack Desktop.
          </div>
          <div className="row">
            <button onClick={() => chrome.runtime.sendMessage({ type: 'localtrack:reconnect' })}>
              Retry
            </button>
          </div>
          <div className="section muted">
            Setup: build the desktop app, then run
            <br />
            <code>localtrack-native-host install {chrome.runtime.id}</code>
          </div>
        </>
      ) : (
        <>
          <div>{status ? status.state.replace('_', ' ') : 'Unknown'}</div>
          <div className="clock">{formatDuration(elapsed)}</div>

          <div className="section">
            <div className="muted">Current page</div>
            <div>{data.state.lastKnownDomain ?? 'No tracked page'}</div>
            {data.state.lastActivityStartedAt ? (
              <div className="muted">
                {formatShort(Date.now() - data.state.lastActivityStartedAt)} on this page
              </div>
            ) : null}
          </div>

          <div className="section">
            <div className="muted">Today</div>
            <div>Active: {status ? formatShort(status.todayActiveMs) : '—'}</div>
          </div>

          <div className="row">
            {status?.state === 'CLOCKED_OUT' ? (
              <button className="primary" onClick={() => sendCommand('clock_in')}>
                Clock In
              </button>
            ) : (
              <>
                {status?.state === 'ON_BREAK' ? (
                  <button className="primary" onClick={() => sendCommand('end_break')}>
                    Resume
                  </button>
                ) : (
                  <button onClick={() => sendCommand('start_break')}>Take Break</button>
                )}
                <button onClick={() => sendCommand('clock_out')}>Clock Out</button>
              </>
            )}
          </div>
        </>
      )}

      {data.queued > 0 ? (
        <div className="section muted">{data.queued} observation(s) waiting to be saved.</div>
      ) : null}
      {data.dropped > 0 ? (
        <div className="warning">Some browser activity could not be saved.</div>
      ) : null}

      <div className="section muted">
        Everything stays on this computer. No account, no cloud, no telemetry.
      </div>
    </div>
  );
}
