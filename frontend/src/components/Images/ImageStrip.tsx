'use client';

import { useCallback, useEffect, useState } from 'react';
import { convertFileSrc } from '@tauri-apps/api/core';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { MeetingImage, listMeetingImages } from '@/lib/capture';
import { ImageViewer } from './ImageViewer';

interface Props {
  /** Meeting folder on disk. */
  folder: string | null | undefined;
  /** Re-check for new snapshots while a meeting is being recorded. */
  live?: boolean;
}

/** "Images N" stack under the notes. Click to expand, click a picture to enlarge. */
export function ImageStrip({ folder, live }: Props) {
  const [images, setImages] = useState<MeetingImage[]>([]);
  const [open, setOpen] = useState(false);
  const [viewing, setViewing] = useState<number | null>(null);

  const load = useCallback(() => {
    if (!folder) return;
    listMeetingImages(folder).then(setImages).catch(() => setImages([]));
  }, [folder]);

  useEffect(() => {
    load();
    if (!live) return;
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, [load, live]);

  if (!folder || images.length === 0) return null;

  const stack = images.slice(-3);

  return (
    <section className="mt-6" aria-label="Images from shared screens">
      <button
        className="flex items-center gap-3 rounded-lg border border-gray-200 bg-white px-3 py-2 text-left hover:bg-gray-50"
        onClick={() => setOpen(o => !o)}
        aria-expanded={open}
      >
        <span className="relative block h-9 w-14" aria-hidden>
          {stack.map((im, i) => (
            // eslint-disable-next-line @next/next/no-img-element
            <img
              key={im.name}
              src={convertFileSrc(im.path)}
              alt=""
              loading="lazy"
              className="absolute top-0 h-9 w-12 rounded border border-white object-cover shadow"
              style={{ left: i * 4, zIndex: i }}
            />
          ))}
        </span>
        <span className="text-sm font-medium text-gray-900">Images {images.length}</span>
        {open ? <ChevronDown className="h-4 w-4 text-gray-500" /> : <ChevronRight className="h-4 w-4 text-gray-500" />}
      </button>

      {open && (
        <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4">
          {images.map((im, i) => (
            <button
              key={im.name}
              className="group overflow-hidden rounded-lg border border-gray-200 bg-white focus:outline-none focus:ring-2 focus:ring-blue-500"
              onClick={() => setViewing(i)}
              aria-label={`Open image ${i + 1}`}
            >
              {/* eslint-disable-next-line @next/next/no-img-element */}
              <img
                src={convertFileSrc(im.path)}
                alt={im.text ? im.text.slice(0, 80) : `Shared screen ${i + 1}`}
                loading="lazy"
                className="aspect-video w-full object-cover transition group-hover:opacity-90"
              />
            </button>
          ))}
        </div>
      )}

      {viewing !== null && (
        <ImageViewer
          folder={folder}
          images={images}
          index={Math.min(viewing, images.length - 1)}
          onIndex={setViewing}
          onClose={() => setViewing(null)}
          onDeleted={load}
        />
      )}
    </section>
  );
}
