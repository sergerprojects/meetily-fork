import { invoke } from '@tauri-apps/api/core';

export interface CalendarAttendee {
  name: string;
  email: string;
  response_status: string | null;
  organizer: boolean;
}

export interface CalendarEvent {
  id: string;
  summary: string;
  start: string;
  end: string;
  attendees: CalendarAttendee[];
  organizer: string | null;
  html_link: string | null;
  location: string | null;
  description: string | null;
  video_url: string | null;
}

export interface CalendarStatus {
  enabled: boolean;
  reason: string | null;
  /** 'ical' (private link) or 'google' (gcloud credentials) when enabled. */
  source?: 'ical' | 'google' | null;
}

export const CALENDAR_MEETING_STARTING_EVENT = 'calendar-meeting-starting';
/** Rust (tray click / auto-record) asks to start a recording. */
export const CALENDAR_RECORD_REQUEST_EVENT = 'calendar-record-request';
/** Rust detected an unmatched call (mic in use) — ask "Record this call?". */
export const CALENDAR_ADHOC_CALL_EVENT = 'calendar-adhoc-call';

export type CalendarRecordMode = 'off' | 'prompt' | 'auto';

// sessionStorage key read by useRecordingStart to name the next recording.
const CALENDAR_TITLE_KEY = 'calendarMeetingTitle';
const CALENDAR_TITLE_MAX_AGE_MS = 2 * 60 * 1000;

export async function getCalendarStatus(): Promise<CalendarStatus> {
  try {
    return await invoke<CalendarStatus>('calendar_status');
  } catch {
    return { enabled: false, reason: 'unavailable' };
  }
}

/** Connect a private iCal link (string) or disconnect it (null). Rejects with a readable message. */
export async function setCalendarIcalUrl(url: string | null): Promise<CalendarStatus> {
  return invoke<CalendarStatus>('calendar_set_ical_url', { url });
}

export async function getUpcomingCalendarEvents(): Promise<CalendarEvent[]> {
  try {
    return await invoke<CalendarEvent[]>('calendar_upcoming');
  } catch {
    return [];
  }
}

export async function getCalendarRecordMode(): Promise<CalendarRecordMode> {
  try {
    return await invoke<CalendarRecordMode>('get_calendar_record_mode');
  } catch {
    return 'prompt';
  }
}

export async function setCalendarRecordMode(mode: CalendarRecordMode): Promise<void> {
  await invoke('set_calendar_record_mode', { mode });
}

/** Don't auto-record or re-prompt this event on join for the rest of the session. */
export async function dismissCalendarEvent(eventId: string): Promise<void> {
  try {
    await invoke('calendar_dismiss_event', { eventId });
  } catch {
    // ignore
  }
}

export function formatEventTime(event: CalendarEvent): string {
  const start = new Date(event.start);
  const end = new Date(event.end);
  const fmt = (d: Date) => d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  return `${fmt(start)} – ${fmt(end)}`;
}

/** Title for the next recording if a calendar Record click just happened; consumed once. */
export function consumeCalendarMeetingTitle(): string | null {
  if (typeof window === 'undefined') return null;
  const raw = sessionStorage.getItem(CALENDAR_TITLE_KEY);
  if (!raw) return null;
  sessionStorage.removeItem(CALENDAR_TITLE_KEY);
  try {
    const { title, at } = JSON.parse(raw) as { title?: string; at?: number };
    if (title && at && Date.now() - at < CALENDAR_TITLE_MAX_AGE_MS) return title;
  } catch {
    // ignore malformed value
  }
  return null;
}

/**
 * Start a recording for a calendar event using the app's existing start flow
 * (same path as the sidebar record button), named after the event.
 */
export async function startRecordingForEvent(
  event: CalendarEvent,
  pathname: string | null,
  navigateHome: () => void
): Promise<void> {
  try {
    await invoke('calendar_set_pending_event', { event });
  } catch (error) {
    console.warn('Failed to remember calendar event for recording:', error);
  }
  startRecordingWithTitle(event.summary, pathname, navigateHome);
}

/** Start a recording named `title` (no calendar event), same start flow. */
export function startRecordingWithTitle(
  title: string,
  pathname: string | null,
  navigateHome: () => void
): void {
  sessionStorage.setItem(CALENDAR_TITLE_KEY, JSON.stringify({ title, at: Date.now() }));

  if (pathname === '/') {
    window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
  } else {
    sessionStorage.setItem('autoStartRecording', 'true');
    navigateHome();
  }
}
