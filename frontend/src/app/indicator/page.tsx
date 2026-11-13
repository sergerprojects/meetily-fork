"use client";

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface IndicatorState {
  recording: boolean;
  paused: boolean;
  secs: number;
}

function clock(total: number): string {
  const s = Math.max(0, Math.floor(total));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = String(m).padStart(2, '0');
  const ss = String(sec).padStart(2, '0');
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

/** Small always-on-top chip shown while recording and Graphite is in the background. */
export default function IndicatorPage() {
  const [state, setState] = useState<IndicatorState>({ recording: false, paused: false, secs: 0 });
  const [stopping, setStopping] = useState(false);

  useEffect(() => {
    const tick = () => invoke<IndicatorState>('indicator_state').then(setState).catch(() => {});
    tick();
    const t = setInterval(tick, 1000);
    return () => clearInterval(t);
  }, []);

  const stop = async () => {
    setStopping(true);
    try {
      await invoke('indicator_stop');
    } finally {
      setStopping(false);
    }
  };

  return (
    <div
      data-tauri-drag-region
      className="flex h-screen w-screen select-none items-center gap-3 rounded-full border border-gray-200 bg-white/95 px-4 shadow-lg"
    >
      <span
        data-tauri-drag-region
        className={`h-2.5 w-2.5 flex-shrink-0 rounded-full ${state.paused ? 'bg-amber-500' : 'animate-pulse bg-red-500'}`}
        aria-hidden
      />
      <span data-tauri-drag-region className="font-mono text-sm tabular-nums text-gray-900">
        {clock(state.secs)}
      </span>
      <span data-tauri-drag-region className="text-xs text-gray-500">
        {state.paused ? 'Paused' : 'Recording'}
      </span>
      <div className="ml-auto flex items-center gap-1.5">
        <button
          onClick={() => invoke('indicator_open')}
          className="rounded-full px-2.5 py-1 text-xs text-gray-700 hover:bg-gray-100"
        >
          Open
        </button>
        <button
          onClick={stop}
          disabled={stopping}
          className="rounded-full bg-gray-900 px-2.5 py-1 text-xs text-white disabled:opacity-50"
        >
          {stopping ? 'Stopping' : 'Stop'}
        </button>
      </div>
    </div>
  );
}
