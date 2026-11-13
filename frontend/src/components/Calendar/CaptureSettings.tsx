'use client';

import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Switch } from '@/components/ui/switch';
import { Button } from '@/components/ui/button';
import {
  CaptureStatus,
  getCaptureStatus,
  openPrivacyPane,
  requestAccessibility,
  requestScreenRecording,
  setCaptureNames,
  setCaptureSnapshots,
} from '@/lib/capture';

function PermissionRow({
  granted,
  label,
  onSetUp,
}: {
  granted: boolean;
  label: string;
  onSetUp: () => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3 text-sm">
      <span className={granted ? 'text-green-700' : 'text-gray-600'}>
        {granted ? `${label}: allowed` : `${label}: not allowed yet`}
      </span>
      {!granted && (
        <Button size="sm" variant="outline" onClick={onSetUp}>
          Set up
        </Button>
      )}
    </div>
  );
}

export function CaptureSettings() {
  const [status, setStatus] = useState<CaptureStatus | null>(null);

  const refresh = useCallback(() => {
    getCaptureStatus().then(setStatus).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    // Permissions are granted in System Settings; re-check when we come back.
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  }, [refresh]);

  const setUpAccessibility = async () => {
    const ok = await requestAccessibility().catch(() => false);
    if (!ok) await openPrivacyPane('Privacy_Accessibility').catch(() => {});
    refresh();
  };

  const setUpScreen = async () => {
    const ok = await requestScreenRecording().catch(() => false);
    if (!ok) await openPrivacyPane('Privacy_ScreenCapture').catch(() => {});
    refresh();
  };

  const toggleNames = async (on: boolean) => {
    try {
      await setCaptureNames(on);
      setStatus(s => (s ? { ...s, names: on } : s));
      if (on && status && !status.accessibility) await setUpAccessibility();
    } catch (e) {
      toast.error(`Could not change this setting: ${e}`);
    }
  };

  const toggleSnapshots = async (on: boolean) => {
    try {
      await setCaptureSnapshots(on);
      setStatus(s => (s ? { ...s, snapshots: on } : s));
      if (on) {
        toast.info('Snapshots keep pictures of what other people share, on this Mac only. Make sure everyone on the call is fine with that.');
        if (status && !status.screen_recording) await setUpScreen();
        if (status && !status.accessibility) await setUpAccessibility();
      }
    } catch (e) {
      toast.error(`Could not change this setting: ${e}`);
    }
  };

  return (
    <div className="space-y-3">
      <div className="p-4 border rounded-lg space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex-1 pr-4">
            <div className="font-medium">Names from the call app</div>
            <div className="text-sm text-gray-600">
              Reads who is on the call and who is speaking from Zoom, Teams or Google Meet, and puts real names on the transcript. Nothing is recorded or sent anywhere.
            </div>
          </div>
          <Switch checked={status?.names ?? true} disabled={!status} onCheckedChange={toggleNames} />
        </div>
        {status && (status.names || !status.accessibility) && (
          <PermissionRow granted={status.accessibility} label="Accessibility" onSetUp={setUpAccessibility} />
        )}
      </div>

      <div className="p-4 border rounded-lg space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex-1 pr-4">
            <div className="font-medium">Snapshots of shared screens</div>
            <div className="text-sm text-gray-600">
              When someone shares their screen, saves pictures of the slides and documents with the meeting. Pictures stay on this Mac. Only other people&apos;s shares are captured, never yours.
            </div>
          </div>
          <Switch checked={status?.snapshots ?? false} disabled={!status} onCheckedChange={toggleSnapshots} />
        </div>
        {status && status.snapshots && (
          <div className="space-y-2">
            <PermissionRow granted={status.screen_recording} label="Screen Recording" onSetUp={setUpScreen} />
            <PermissionRow granted={status.accessibility} label="Accessibility" onSetUp={setUpAccessibility} />
          </div>
        )}
      </div>
    </div>
  );
}
