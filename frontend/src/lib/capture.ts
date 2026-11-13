import { invoke } from '@tauri-apps/api/core';

export interface CaptureStatus {
  accessibility: boolean;
  screen_recording: boolean;
  names: boolean;
  snapshots: boolean;
}

export interface MeetingImage {
  name: string;
  /** Absolute path, usable with convertFileSrc. */
  path: string;
  /** Seconds into the recording when it was taken. */
  at: number | null;
  /** Text read off the image. */
  text: string | null;
}

export const getCaptureStatus = () => invoke<CaptureStatus>('capture_status');
export const requestAccessibility = () => invoke<boolean>('capture_request_accessibility');
export const requestScreenRecording = () => invoke<boolean>('capture_request_screen');
export const setCaptureNames = (enabled: boolean) => invoke<void>('set_capture_names', { enabled });
export const setCaptureSnapshots = (enabled: boolean) => invoke<void>('set_capture_snapshots', { enabled });

export const listMeetingImages = (folder: string) =>
  invoke<MeetingImage[]>('list_meeting_images', { folder });
export const readMeetingImage = (folder: string, name: string) =>
  invoke<number[]>('read_meeting_image', { folder, name });
export const deleteMeetingImage = (folder: string, name: string) =>
  invoke<void>('delete_meeting_image', { folder, name });
export const readMeetingNotes = (folder: string) => invoke<string>('read_meeting_notes', { folder });

/** Opens the macOS privacy pane for a permission. */
export const openPrivacyPane = (pane: 'Privacy_Accessibility' | 'Privacy_ScreenCapture') =>
  invoke<void>('open_system_settings', { preferencePane: pane });

/** Saves the accessibility tree of a running call app; returns the file path. */
export const probeCallApp = (app: 'zoom' | 'teams' | 'chrome') =>
  invoke<string>('capture_ax_probe', { app });

/** Copy one snapshot into Downloads; resolves to the new file's path. */
export const exportMeetingImage = (folder: string, name: string) =>
  invoke<string>('export_meeting_image', { folder, name });
