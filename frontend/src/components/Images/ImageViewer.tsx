'use client';

import { useCallback, useEffect, useState } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { ChevronLeft, ChevronRight, Copy, Download, Trash2, X } from 'lucide-react';
import { MeetingImage, deleteMeetingImage, exportMeetingImage, readMeetingImage } from '@/lib/capture';

function clock(sec: number | null): string {
  if (sec == null) return '';
  const s = Math.max(0, Math.floor(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

interface Props {
  folder: string;
  images: MeetingImage[];
  index: number;
  onIndex: (i: number) => void;
  onClose: () => void;
  onDeleted: () => void;
}

/** Full-size view of one snapshot: previous / next, copy, download, remove, close. */
export function ImageViewer({ folder, images, index, onIndex, onClose, onDeleted }: Props) {
  const img = images[index];
  const [showText, setShowText] = useState(false);

  const go = useCallback(
    (d: number) => {
      if (images.length === 0) return;
      onIndex((index + d + images.length) % images.length);
    },
    [images.length, index, onIndex]
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
      else if (e.key === 'ArrowLeft') go(-1);
      else if (e.key === 'ArrowRight') go(1);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [go, onClose]);

  if (!img) return null;

  const copy = async () => {
    try {
      const bytes = await readMeetingImage(folder, img.name);
      const blob = new Blob([new Uint8Array(bytes)], { type: 'image/png' });
      await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
      toast.success('Image copied');
    } catch (e) {
      toast.error(`Could not copy the image: ${e}`);
    }
  };

  const download = async () => {
    try {
      const dest = await exportMeetingImage(folder, img.name);
      toast.success(`Saved to Downloads: ${dest.split('/').pop()}`);
    } catch (e) {
      toast.error(`Could not save the image: ${e}`);
    }
  };

  const remove = async () => {
    try {
      await deleteMeetingImage(folder, img.name);
      onDeleted();
      if (images.length <= 1) onClose();
      else onIndex(Math.min(index, images.length - 2));
    } catch (e) {
      toast.error(`Could not remove the image: ${e}`);
    }
  };

  const btn = 'rounded-full bg-white/90 p-2 text-gray-800 shadow hover:bg-white focus:outline-none focus:ring-2 focus:ring-white';

  return (
    <div
      className="fixed inset-0 z-50 flex flex-col bg-black/80 backdrop-blur-sm"
      role="dialog"
      aria-label="Shared screen snapshot"
      onClick={onClose}
    >
      <div className="flex items-center justify-between p-4 text-white" onClick={e => e.stopPropagation()}>
        <div className="text-sm">
          {index + 1} of {images.length}
          {img.at != null && <span className="ml-3 text-white/70">{clock(img.at)} into the meeting</span>}
        </div>
        <div className="flex items-center gap-2">
          <button className={btn} onClick={copy} aria-label="Copy image" title="Copy">
            <Copy className="h-4 w-4" />
          </button>
          <button className={btn} onClick={download} aria-label="Download image" title="Download">
            <Download className="h-4 w-4" />
          </button>
          <button className={btn} onClick={remove} aria-label="Remove image" title="Remove">
            <Trash2 className="h-4 w-4" />
          </button>
          <button className={btn} onClick={onClose} aria-label="Close" title="Close">
            <X className="h-4 w-4" />
          </button>
        </div>
      </div>

      <div className="relative flex min-h-0 flex-1 items-center justify-center px-16 pb-4">
        {images.length > 1 && (
          <button
            className={`${btn} absolute left-4 top-1/2 -translate-y-1/2`}
            onClick={e => {
              e.stopPropagation();
              go(-1);
            }}
            aria-label="Previous image"
          >
            <ChevronLeft className="h-5 w-5" />
          </button>
        )}
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={convertFileSrc(img.path)}
          alt={img.text ? img.text.slice(0, 120) : 'Shared screen'}
          className="max-h-full max-w-full rounded-md bg-white object-contain shadow-2xl"
          onClick={e => e.stopPropagation()}
        />
        {images.length > 1 && (
          <button
            className={`${btn} absolute right-4 top-1/2 -translate-y-1/2`}
            onClick={e => {
              e.stopPropagation();
              go(1);
            }}
            aria-label="Next image"
          >
            <ChevronRight className="h-5 w-5" />
          </button>
        )}
      </div>

      {img.text && (
        <div className="px-6 pb-4 text-white" onClick={e => e.stopPropagation()}>
          <button className="text-xs underline text-white/80" onClick={() => setShowText(v => !v)}>
            {showText ? 'Hide text from this image' : 'Show text from this image'}
          </button>
          {showText && (
            <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap rounded bg-black/40 p-3 text-xs text-white/90">
              {img.text}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
