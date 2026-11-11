import Image from 'next/image';
import { AppVersion } from './AppVersion';

export function About() {
  return (
    <div className="p-4 space-y-4">
      <div className="text-center">
        <Image src="/icon.png" alt="Peacockery Meetily" width={64} height={64} className="mx-auto mb-3" />
        <h1 className="text-xl font-bold text-gray-900">Peacockery Meetily</h1>
        <AppVersion className="text-sm text-gray-500" />
        <p className="text-medium text-gray-600 mt-2">
          Records the microphone and call audio on this PC. Live and final transcripts and summaries
          run on the Peacockery Voice API.
        </p>
      </div>
      <div className="grid grid-cols-1 gap-2 text-sm text-gray-700">
        <div className="bg-gray-50 rounded p-3">
          <span className="font-semibold">Live transcript</span> while you talk, labelled Me and Them.
        </div>
        <div className="bg-gray-50 rounded p-3">
          <span className="font-semibold">Final transcript</span> after stop, with speaker labels.
        </div>
        <div className="bg-gray-50 rounded p-3">
          <span className="font-semibold">Summary</span> from a template, editable in place.
        </div>
      </div>
      <p className="pt-2 border-t border-gray-200 text-center text-xs text-gray-400">
        Internal build, forked from the open-source Meetily project (MIT).
      </p>
    </div>
  );
}
