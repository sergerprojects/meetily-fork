'use client';

import { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart';
import { Switch } from '@/components/ui/switch';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  CalendarEvent,
  CalendarRecordMode,
  CalendarStatus,
  formatEventTime,
  getCalendarRecordMode,
  getAppleCalendarStatus,
  getCalendarStatus,
  useAppleCalendar,
  getIgnoredApps,
  getSilenceStopMinutes,
  setSilenceStopMinutes,
  setIgnoredApps,
  getUpcomingCalendarEvents,
  setCalendarIcalUrl,
  setCalendarRecordMode,
} from '@/lib/calendar';

const MODE_LABELS: Record<CalendarRecordMode, string> = {
  off: 'Off',
  prompt: 'Ask me',
  auto: 'Record automatically',
};

/** Settings > Recording: calendar status, auto-record mode, next 3 events. */
export function CalendarSettings() {
  const [status, setStatus] = useState<CalendarStatus | null>(null);
  const [mode, setMode] = useState<CalendarRecordMode>('prompt');
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [launchAtLogin, setLaunchAtLogin] = useState<boolean | null>(null);
  const [linkInput, setLinkInput] = useState('');
  const [connecting, setConnecting] = useState(false);
  const [ignoredText, setIgnoredText] = useState('');
  const [appleStatus, setAppleStatus] = useState('unavailable');
  const [appleBusy, setAppleBusy] = useState(false);
  const [silenceMinutes, setSilenceMinutes] = useState('10');

  useEffect(() => {
    isEnabled().then(setLaunchAtLogin).catch(() => setLaunchAtLogin(null));
    getIgnoredApps().then(apps => setIgnoredText(apps.join(', ')));
    getAppleCalendarStatus().then(setAppleStatus);
    getSilenceStopMinutes().then(m => setSilenceMinutes(String(m)));
  }, []);

  const handleLaunchAtLogin = async (checked: boolean) => {
    const previous = launchAtLogin;
    setLaunchAtLogin(checked);
    try {
      await (checked ? enable() : disable());
    } catch (error) {
      setLaunchAtLogin(previous);
      toast.error('Failed to change launch at login', { description: String(error) });
    }
  };

  useEffect(() => {
    let active = true;
    (async () => {
      const [s, m, upcoming] = await Promise.all([
        getCalendarStatus(),
        getCalendarRecordMode(),
        getUpcomingCalendarEvents(),
      ]);
      if (!active) return;
      setStatus(s);
      setMode(m);
      setEvents(upcoming.slice(0, 3));
    })();
    return () => {
      active = false;
    };
  }, []);

  const refreshEvents = async () => {
    const [s, upcoming] = await Promise.all([getCalendarStatus(), getUpcomingCalendarEvents()]);
    setStatus(s);
    setEvents(upcoming.slice(0, 3));
  };

  const handleUseApple = async () => {
    setAppleBusy(true);
    try {
      await useAppleCalendar();
      toast.success('Using your Mac Calendar app');
      setAppleStatus('granted');
      await refreshEvents();
    } catch (error) {
      toast.error('Could not use the Calendar app', { description: String(error) });
      setAppleStatus(await getAppleCalendarStatus());
    } finally {
      setAppleBusy(false);
    }
  };

  const handleConnect = async () => {
    setConnecting(true);
    try {
      await setCalendarIcalUrl(linkInput);
      setLinkInput('');
      toast.success('Google Calendar connected');
      await refreshEvents();
    } catch (error) {
      toast.error('Could not connect calendar', { description: String(error) });
    } finally {
      setConnecting(false);
    }
  };

  const handleDisconnect = async () => {
    try {
      await setCalendarIcalUrl(null);
      await refreshEvents();
    } catch (error) {
      toast.error('Could not remove calendar link', { description: String(error) });
    }
  };

  const handleSilenceChange = async (value: string) => {
    const previous = silenceMinutes;
    setSilenceMinutes(value);
    try {
      await setSilenceStopMinutes(Number(value));
    } catch (error) {
      setSilenceMinutes(previous);
      toast.error('Failed to save', { description: String(error) });
    }
  };

  const saveIgnoredApps = async () => {
    const apps = ignoredText.split(',').map(a => a.trim()).filter(Boolean);
    try {
      await setIgnoredApps(apps);
      setIgnoredText(apps.join(', '));
      toast.success('Saved');
    } catch (error) {
      toast.error('Failed to save', { description: String(error) });
    }
  };

  const handleModeChange = async (value: string) => {
    const next = value as CalendarRecordMode;
    const previous = mode;
    setMode(next);
    try {
      await setCalendarRecordMode(next);
    } catch (error) {
      setMode(previous);
      toast.error('Failed to save auto-record setting', { description: String(error) });
    }
  };

  return (
    <div className="border-t pt-6 space-y-4">
      <div>
        <h4 className="text-base font-medium text-gray-900 mb-1">Calendar &amp; auto-record</h4>
        <p className="text-sm text-gray-600">
          When you join a call (microphone in use) during a calendar meeting with a video link, Meetily can ask to record or start recording automatically. Calls without a matching meeting only ever get a prompt.
        </p>
      </div>

      <div className="flex items-center justify-between p-4 border rounded-lg">
        <div className="flex-1 pr-4">
          <div className="font-medium">Launch at login</div>
          <div className="text-sm text-gray-600">
            Starts Meetily in the menu bar when you log in, so meeting prompts and auto-record work.
          </div>
        </div>
        <Switch
          checked={launchAtLogin === true}
          disabled={launchAtLogin === null}
          onCheckedChange={handleLaunchAtLogin}
        />
      </div>

      <div className="p-4 border rounded-lg bg-gray-50 space-y-3">
        <div className="font-medium">Google Calendar</div>
        <div className="text-sm text-gray-600 break-words">
          {status === null
            ? 'Checking…'
            : status.enabled
              ? `Connected via ${status.source === 'ical' ? 'private iCal link' : status.source === 'apple' ? 'the Mac Calendar app' : 'Google sign-in (gcloud)'}${status.reason ? ` (last sync error: ${status.reason})` : ''}`
              : `Not connected${status.reason ? ` — ${status.reason}` : ''}`}
        </div>
        {appleStatus !== 'unavailable' && status?.source !== 'apple' && (
          <div className="flex items-center justify-between gap-3 rounded-lg border border-blue-100 bg-blue-50 p-3">
            <div className="text-sm text-gray-700">
              <div className="font-medium">Use the Calendar app on this Mac</div>
              <div className="text-xs text-gray-500">
                Works with any account in your Calendar app (Google, Exchange, iCloud). No Google sign-in or cloud project needed.
              </div>
            </div>
            <Button size="sm" onClick={handleUseApple} disabled={appleBusy}>
              {appleBusy ? 'Waiting…' : appleStatus === 'denied' ? 'Try again' : 'Use it'}
            </Button>
          </div>
        )}
        {status?.source === 'ical' ? (
          <Button variant="outline" size="sm" onClick={handleDisconnect}>
            Remove link
          </Button>
        ) : (
          <div className="space-y-2">
            <p className="text-xs text-gray-500">
              In Google Calendar open Settings → your calendar → Integrate calendar and copy the{' '}
              <b>Secret address in iCal format</b>. Treat it like a password: anyone with it can read your calendar. It is stored only on this Mac. New or moved meetings can take a few hours to appear.
            </p>
            <div className="flex gap-2">
              <Input
                type="password"
                autoComplete="off"
                placeholder="https://calendar.google.com/calendar/ical/…/basic.ics"
                value={linkInput}
                onChange={e => setLinkInput(e.target.value)}
              />
              <Button size="sm" onClick={handleConnect} disabled={connecting || !linkInput.trim()}>
                {connecting ? 'Checking…' : 'Connect'}
              </Button>
            </div>
          </div>
        )}
      </div>

      <div className="flex items-center justify-between p-4 border rounded-lg">
        <div className="flex-1 pr-4">
          <div className="font-medium">When I join a meeting</div>
          <div className="text-sm text-gray-600">
            Automatic recordings stop when the call ends or 10 minutes after the meeting&apos;s end time.
          </div>
        </div>
        <Select value={mode} onValueChange={handleModeChange}>
          <SelectTrigger className="w-[200px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {(Object.keys(MODE_LABELS) as CalendarRecordMode[]).map(m => (
              <SelectItem key={m} value={m}>
                {MODE_LABELS[m]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="flex items-center justify-between p-4 border rounded-lg">
        <div className="flex-1 pr-4">
          <div className="font-medium">Stop recording when nobody is talking</div>
          <div className="text-sm text-gray-600">
            Ends and saves a recording after this long with no speech in the transcript. Never while paused.
          </div>
        </div>
        <Select value={silenceMinutes} onValueChange={handleSilenceChange}>
          <SelectTrigger className="w-[200px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {[['0', 'Never'], ['3', 'After 3 minutes'], ['5', 'After 5 minutes'], ['10', 'After 10 minutes'], ['15', 'After 15 minutes'], ['30', 'After 30 minutes']].map(([v, label]) => (
              <SelectItem key={v} value={v}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="p-4 border rounded-lg space-y-2">
        <div className="font-medium">Never treat these apps as a meeting</div>
        <div className="text-sm text-gray-600">
          Comma-separated app names (for example Discord, FaceTime). Using the microphone in these apps won&apos;t ask to record.
        </div>
        <div className="flex gap-2">
          <Input
            placeholder="Discord, FaceTime"
            value={ignoredText}
            onChange={e => setIgnoredText(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && saveIgnoredApps()}
          />
          <Button size="sm" variant="outline" onClick={saveIgnoredApps}>
            Save
          </Button>
        </div>
      </div>

      {status?.enabled && (
        <div className="p-4 border rounded-lg">
          <div className="font-medium mb-2">Next meetings</div>
          {events.length === 0 ? (
            <div className="text-sm text-gray-600">No upcoming meetings in the next 12 hours.</div>
          ) : (
            <ul className="space-y-1">
              {events.map(event => (
                <li key={event.id} className="flex items-center justify-between gap-3 text-sm">
                  <span className="truncate text-gray-900">
                    {event.video_url ? '● ' : ''}
                    {event.summary}
                  </span>
                  <span className="shrink-0 text-gray-500">{formatEventTime(event)}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
