import React, { useState, useEffect } from "react";
import { getVersion } from '@tauri-apps/api/app';
import Image from 'next/image';

export function About() {
    const [currentVersion, setCurrentVersion] = useState<string>('');

    useEffect(() => {
        getVersion().then(setCurrentVersion).catch(console.error);
    }, []);

    return (
        <div className="p-4 space-y-3 text-center">
            <Image
                src="icon_128x128.png"
                alt="Graphite"
                width={64}
                height={64}
                className="mx-auto"
            />
            <div>
                <h2 className="text-lg font-semibold text-gray-900">Graphite</h2>
                <span className="text-sm text-gray-500">Version {currentVersion}</span>
            </div>
            <p className="text-sm text-gray-600">
                Meeting notes and transcripts that stay on this Mac.
            </p>
            <p className="text-xs text-gray-400">
                Graphite is a personal fork of Meetily by Zackriya Solutions, used under the MIT License.
            </p>
        </div>
    )
}
