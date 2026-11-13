'use client';

import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Switch } from '@/components/ui/switch';

/** "Remind me to tell everyone I'm recording" toast shown when a recording starts. */
export function ParticipantReminder() {
  const [on, setOn] = useState(true);

  useEffect(() => {
    (async () => {
      try {
        const { Store } = await import('@tauri-apps/plugin-store');
        const store = await Store.load('preferences.json');
        setOn((await store.get<boolean>('show_recording_notification')) ?? true);
      } catch (e) {
        console.error('Failed to load reminder preference:', e);
      }
    })();
  }, []);

  const change = async (enabled: boolean) => {
    setOn(enabled);
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('preferences.json');
      await store.set('show_recording_notification', enabled);
      await store.save();
    } catch (e) {
      console.error('Failed to save reminder preference:', e);
      toast.error('Could not save this setting');
    }
  };

  return (
    <div className="flex items-center justify-between rounded-lg border p-4">
      <div className="flex-1 pr-4">
        <div className="font-medium">Remind me to tell everyone I&apos;m recording</div>
        <div className="text-sm text-gray-600">A short reminder appears when a recording starts. Recording can need everyone&apos;s consent.</div>
      </div>
      <Switch checked={on} onCheckedChange={change} />
    </div>
  );
}
