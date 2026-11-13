'use client';

import { useEffect, useRef } from 'react';
import { listen } from '@tauri-apps/api/event';
import { usePathname, useRouter } from 'next/navigation';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import {
  CALENDAR_ADHOC_CALL_EVENT,
  CALENDAR_MEETING_STARTING_EVENT,
  CALENDAR_RECORD_REQUEST_EVENT,
  CalendarEvent,
  dismissCalendarEvent,
  formatEventTime,
  startRecordingForEvent,
  startRecordingWithTitle,
} from '@/lib/calendar';

/**
 * Listens for Rust calendar/auto-record events:
 * - `calendar-meeting-starting`: non-blocking "Record" prompt (poller or join).
 * - `calendar-adhoc-call`: "Record this call?" prompt for unmatched calls.
 * - `calendar-record-request`: start now (tray event click or auto mode),
 *   through the same start flow as the Record button.
 * Renders nothing.
 */
export function CalendarMeetingPrompt() {
  const pathname = usePathname();
  const router = useRouter();
  const { isRecording } = useRecordingState();

  // Keep latest values for the long-lived listener.
  const pathnameRef = useRef(pathname);
  const isRecordingRef = useRef(isRecording);
  pathnameRef.current = pathname;
  isRecordingRef.current = isRecording;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    (async () => {
      const goHome = () => router.push('/');

      const unlistenStarting = await listen<{ event: CalendarEvent }>(CALENDAR_MEETING_STARTING_EVENT, ({ payload }) => {
        const event = payload?.event;
        if (!event || isRecordingRef.current) return;

        const attendeeCount = event.attendees?.length ?? 0;
        toast(`${event.summary} is starting`, {
          id: `calendar-${event.id}`,
          description: `${formatEventTime(event)}${attendeeCount > 0 ? ` · ${attendeeCount} attendee${attendeeCount === 1 ? '' : 's'}` : ''}`,
          duration: 10 * 60 * 1000,
          action: {
            label: 'Record',
            onClick: () => {
              startRecordingForEvent(event, pathnameRef.current, goHome);
            },
          },
          cancel: {
            label: 'Dismiss',
            onClick: () => {
              dismissCalendarEvent(event.id);
            },
          },
        });
      });

      const unlistenAdhoc = await listen<{ title: string }>(CALENDAR_ADHOC_CALL_EVENT, ({ payload }) => {
        const title = payload?.title;
        if (!title || isRecordingRef.current) return;
        toast('Record this call?', {
          id: 'calendar-adhoc-call',
          description: title,
          duration: 2 * 60 * 1000,
          action: {
            label: 'Record',
            onClick: () => startRecordingWithTitle(title, pathnameRef.current, goHome),
          },
          cancel: { label: 'Dismiss', onClick: () => {} },
        });
      });

      const unlistenRequest = await listen<{ event: CalendarEvent | null; title: string }>(
        CALENDAR_RECORD_REQUEST_EVENT,
        ({ payload }) => {
          if (!payload || isRecordingRef.current) return;
          if (payload.event) {
            toast.dismiss(`calendar-${payload.event.id}`);
            startRecordingForEvent(payload.event, pathnameRef.current, goHome);
          } else if (payload.title) {
            startRecordingWithTitle(payload.title, pathnameRef.current, goHome);
          }
        }
      );

      const fns = [unlistenStarting, unlistenAdhoc, unlistenRequest];
      if (cancelled) fns.forEach(fn => fn());
      else unlisten = () => fns.forEach(fn => fn());
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [router]);

  return null;
}
