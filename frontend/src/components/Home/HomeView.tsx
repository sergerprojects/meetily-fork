'use client';

import { useEffect, useMemo, useState } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { Mic, RefreshCw } from 'lucide-react';
import {
  CalendarEvent,
  CalendarWeek,
  HomeMeeting,
  getCalendarWeek,
  getHomeMeetings,
  refreshCalendar,
  startRecordingForEvent,
} from '@/lib/calendar';

const REFRESH_MS = 60 * 1000;

const timeOf = (iso: string) => new Date(iso).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });

function dayKey(d: Date) {
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

function dayLabel(d: Date, now: Date) {
  const offset = Math.round((new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() - new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()) / 86_400_000);
  if (offset === 0) return 'Today';
  if (offset === 1) return 'Tomorrow';
  if (offset === -1) return 'Yesterday';
  return d.toLocaleDateString([], { weekday: 'long', month: 'short', day: 'numeric' });
}

function initials(name: string) {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return '?';
  return ((parts[0][0] ?? '') + (parts.length > 1 ? parts[parts.length - 1][0] : '')).toUpperCase();
}

function whoLine(names: string[]) {
  if (names.length === 0) return 'Me';
  const shown = names.slice(0, 2).join(', ');
  const rest = names.length - 2;
  return rest > 0 ? `${shown} & ${rest} other${rest === 1 ? '' : 's'}` : names.join(' & ');
}

/** Home: your whole calendar (today through next week) with notes status, then notes with no calendar event. */
export function HomeView() {
  const router = useRouter();
  const pathname = usePathname();
  const [week, setWeek] = useState<CalendarWeek | null>(null);
  const [meetings, setMeetings] = useState<HomeMeeting[]>([]);
  const [now, setNow] = useState(() => new Date());
  const [refreshing, setRefreshing] = useState(false);

  useEffect(() => {
    let active = true;
    const refresh = async () => {
      const [w, saved] = await Promise.all([getCalendarWeek(), getHomeMeetings(200)]);
      if (!active) return;
      setWeek(w);
      setMeetings(saved);
      setNow(new Date());
    };
    refresh();
    const timer = setInterval(refresh, REFRESH_MS);
    return () => {
      active = false;
      clearInterval(timer);
    };
  }, []);

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      setWeek(await refreshCalendar());
      setMeetings(await getHomeMeetings(200));
      setNow(new Date());
    } finally {
      setRefreshing(false);
    }
  };

  // Recordings per calendar event, newest first.
  const byEvent = useMemo(() => {
    const m = new Map<string, HomeMeeting[]>();
    for (const x of meetings) {
      if (!x.event_id) continue;
      m.set(x.event_id, [...(m.get(x.event_id) ?? []), x]);
    }
    return m;
  }, [meetings]);

  const days = useMemo(() => {
    const groups: { label: string; events: CalendarEvent[] }[] = [];
    for (const e of week?.events ?? []) {
      const label = dayLabel(new Date(e.start), now);
      const last = groups[groups.length - 1];
      if (last && last.label === label) last.events.push(e);
      else groups.push({ label, events: [e] });
    }
    return groups;
  }, [week, now]);

  const orphanDays = useMemo(() => {
    const groups: { label: string; items: HomeMeeting[] }[] = [];
    const sorted = meetings.filter(m => !m.event_id).sort((a, b) => +new Date(b.when) - +new Date(a.when));
    for (const m of sorted) {
      const label = dayLabel(new Date(m.when), now);
      const last = groups[groups.length - 1];
      if (last && last.label === label) last.items.push(m);
      else groups.push({ label, items: [m] });
    }
    return groups;
  }, [meetings, now]);

  const isNow = (e: CalendarEvent) => +new Date(e.start) <= +now && +new Date(e.end) > +now;
  const isPast = (e: CalendarEvent) => +new Date(e.end) <= +now;

  const open = (e: CalendarEvent) => {
    const linked = byEvent.get(e.id);
    if (linked && linked.length > 0) router.push(`/meeting-details?id=${linked[0].id}`);
    else router.push(`/event?id=${encodeURIComponent(e.id)}`);
  };

  const updated = week?.updated_at ? new Date(week.updated_at) : null;

  return (
    <div className="w-full overflow-y-auto bg-[#f7f6f1] pb-40">
      <div className="mx-auto max-w-[680px] px-6 pt-10">
        <div className="mb-4 flex items-end justify-between gap-3">
          <h1 className="font-display text-3xl font-semibold tracking-[-0.005em] text-gray-900">Calendar</h1>
          <div className="flex items-center gap-2 pb-1 text-xs text-gray-500">
            {updated && <span>Updated {updated.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</span>}
            <button
              onClick={handleRefresh}
              disabled={refreshing}
              className="inline-flex items-center gap-1 rounded-full border border-gray-200 bg-white px-2.5 py-1 text-gray-700 hover:bg-gray-50 disabled:opacity-50"
              aria-label="Refresh calendar"
            >
              <RefreshCw className={`h-3 w-3 ${refreshing ? 'animate-spin' : ''}`} />
              {refreshing ? 'Refreshing…' : 'Refresh'}
            </button>
          </div>
        </div>

        {week && !week.enabled && (
          <div className="mb-4 rounded-xl border border-amber-200 bg-amber-50 p-3 text-sm text-amber-900">
            Your calendar isn't connected. Open Settings → Recording → Google Calendar.
          </div>
        )}
        {week?.enabled && week.error && (
          <div className="mb-4 rounded-xl border border-amber-200 bg-amber-50 p-3 text-sm text-amber-900">
            Couldn't refresh your calendar: {week.error}. Showing the last list we have.
          </div>
        )}

        {week?.enabled && days.length === 0 && !week.error && (
          <div className="rounded-2xl border border-gray-200 bg-white p-5 text-sm text-gray-500">
            {updated ? 'No meetings on your calendar this week.' : 'Loading your calendar…'}
          </div>
        )}

        {days.map(group => (
          <section key={group.label} className="mb-6">
            <div className="mb-2 text-sm text-gray-500">{group.label}</div>
            <ul className="overflow-hidden rounded-2xl border border-gray-200 bg-white shadow-sm">
              {group.events.map((e, i) => {
                const linked = byEvent.get(e.id) ?? [];
                const people = e.attendees.filter(a => !a.is_self).map(a => a.name);
                return (
                  <li key={e.id} className={i > 0 ? 'border-t border-gray-100' : ''}>
                    <div className={`group flex items-center gap-3 px-4 py-3 hover:bg-gray-50 ${isPast(e) && linked.length === 0 ? 'opacity-70' : ''}`}>
                      <button className="flex min-w-0 flex-1 items-center gap-3 border-l-4 border-rose-900 pl-3 text-left" onClick={() => open(e)}>
                        <div className="min-w-0 flex-1">
                          <div className="truncate text-[15px] font-medium text-gray-900">{e.summary}</div>
                          <div className="truncate text-xs text-gray-500">
                            {isNow(e) && <span className="mr-1 font-semibold text-green-700">Now ·</span>}
                            {timeOf(e.start)} – {timeOf(e.end)}
                            {people.length > 0 && ` · ${whoLine(people)}`}
                          </div>
                        </div>
                        {linked.length > 0 && (
                          <span className="shrink-0 rounded-full bg-emerald-50 px-2 py-0.5 text-xs text-emerald-800">
                            Notes{linked.length > 1 ? ` · ${linked.length}` : ''}
                          </span>
                        )}
                      </button>
                      <button
                        className="flex h-8 shrink-0 items-center gap-1.5 rounded-full border border-gray-200 px-3 text-xs text-gray-700 hover:bg-white"
                        onClick={() => startRecordingForEvent(e, pathname, () => router.push('/'))}
                        title={linked.length > 0 ? 'Continue recording this meeting' : 'Start recording this meeting'}
                      >
                        <Mic className="h-3.5 w-3.5" />
                        {linked.length > 0 ? 'Continue' : isNow(e) ? 'Start now' : 'Record'}
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}

        {orphanDays.length > 0 && <h2 className="mb-3 mt-10 font-display text-xl font-semibold text-gray-900">Other notes</h2>}
        {orphanDays.map(group => (
          <section key={group.label} className="mb-6">
            <div className="mb-2 text-sm text-gray-500">{group.label}</div>
            <ul>
              {group.items.map(m => (
                <li key={m.id}>
                  <button
                    className="flex w-full items-center gap-3 rounded-lg px-2 py-2.5 text-left hover:bg-white"
                    onClick={() => router.push(`/meeting-details?id=${m.id}`)}
                  >
                    <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-gray-200 text-xs font-medium text-gray-600">
                      {initials(m.attendees[0] ?? 'Me')}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[15px] text-gray-900">{m.title}</div>
                      <div className="truncate text-xs text-gray-500">{whoLine(m.attendees)}</div>
                    </div>
                    <div className="shrink-0 text-xs text-gray-500">{timeOf(m.when)}</div>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </div>
  );
}
