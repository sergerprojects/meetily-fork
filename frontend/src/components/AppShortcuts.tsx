'use client';

import { useCallback, useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from '@/components/ui/command';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useRecordingState } from '@/contexts/RecordingStateContext';

/** Event the live view listens to for the Notes / Transcript switch. */
export const VIEW_EVENT = 'graphite-view';

/**
 * App-wide keyboard shortcuts and the command palette.
 *   ⌘N new recording   ⌘. stop   ⌘, settings   ⌘K palette   ⌘F search
 *   ⌘1 notes   ⌘2 transcript   ⌘⌥S sidebar
 */
export function AppShortcuts() {
  const router = useRouter();
  const { meetings, toggleCollapse, setCurrentMeeting } = useSidebar();
  const { isRecording } = useRecordingState();
  const [open, setOpen] = useState(false);

  const startRecording = useCallback(() => {
    window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
  }, []);

  const stopRecording = useCallback(() => {
    (window as unknown as { handleRecordingStop?: (callApi: boolean) => void }).handleRecordingStop?.(true);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.ctrlKey) return;
      const key = e.key.toLowerCase();
      const run = (fn: () => void) => {
        e.preventDefault();
        fn();
      };
      if (key === 'k') run(() => setOpen(o => !o));
      else if (key === 'n' && !e.altKey && !e.shiftKey) run(() => !isRecording && startRecording());
      else if (key === '.') run(() => isRecording && stopRecording());
      else if (key === ',') run(() => router.push('/settings'));
      else if (key === 'f' && !e.altKey) run(() => setOpen(true));
      else if (key === '1') run(() => window.dispatchEvent(new CustomEvent(VIEW_EVENT, { detail: 'notes' })));
      else if (key === '2') run(() => window.dispatchEvent(new CustomEvent(VIEW_EVENT, { detail: 'transcript' })));
      else if (e.altKey && (key === 's' || key === 'ß')) run(toggleCollapse);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isRecording, router, startRecording, stopRecording, toggleCollapse]);

  const go = (fn: () => void) => {
    setOpen(false);
    // Let the dialog close before navigating so focus lands on the new page.
    setTimeout(fn, 0);
  };

  return (
    <CommandDialog open={open} onOpenChange={setOpen}>
      <CommandInput placeholder="Search meetings or type a command" />
      <CommandList>
        <CommandEmpty>Nothing matches.</CommandEmpty>
        <CommandGroup heading="Actions">
          {isRecording ? (
            <CommandItem onSelect={() => go(stopRecording)}>
              Stop recording
              <CommandShortcut>⌘.</CommandShortcut>
            </CommandItem>
          ) : (
            <CommandItem onSelect={() => go(startRecording)}>
              Start recording
              <CommandShortcut>⌘N</CommandShortcut>
            </CommandItem>
          )}
          <CommandItem onSelect={() => go(() => router.push('/'))}>Home</CommandItem>
          <CommandItem onSelect={() => go(() => router.push('/settings'))}>
            Settings
            <CommandShortcut>⌘,</CommandShortcut>
          </CommandItem>
          <CommandItem onSelect={() => go(toggleCollapse)}>
            Show or hide the sidebar
            <CommandShortcut>⌘⌥S</CommandShortcut>
          </CommandItem>
          <CommandItem onSelect={() => go(() => window.dispatchEvent(new CustomEvent(VIEW_EVENT, { detail: 'notes' })))}>
            Notes view
            <CommandShortcut>⌘1</CommandShortcut>
          </CommandItem>
          <CommandItem onSelect={() => go(() => window.dispatchEvent(new CustomEvent(VIEW_EVENT, { detail: 'transcript' })))}>
            Transcript view
            <CommandShortcut>⌘2</CommandShortcut>
          </CommandItem>
        </CommandGroup>
        {meetings.length > 0 && (
          <CommandGroup heading="Meetings">
            {meetings.slice(0, 100).map(m => (
              <CommandItem
                key={m.id}
                value={`${m.title} ${m.id}`}
                onSelect={() =>
                  go(() => {
                    setCurrentMeeting({ id: m.id, title: m.title });
                    router.push(`/meeting-details?id=${m.id}`);
                  })
                }
              >
                {m.title}
              </CommandItem>
            ))}
          </CommandGroup>
        )}
      </CommandList>
    </CommandDialog>
  );
}
