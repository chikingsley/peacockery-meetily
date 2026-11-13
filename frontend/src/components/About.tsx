import Image from "next/image";

import { AppVersion } from "./AppVersion";

export function About() {
  return (
    <div className="space-y-4 p-4">
      <div className="text-center">
        <Image
          src="/icon.png"
          alt="Peacockery Meetily"
          width={64}
          height={64}
          className="mx-auto mb-3"
        />
        <h1 className="text-xl font-bold text-foreground">
          Peacockery Meetily
        </h1>
        <AppVersion className="text-sm text-muted-foreground" />
        <p className="mt-2 text-sm text-muted-foreground">
          Records the microphone and call audio on this PC. Live and final
          transcripts and summaries run on the Peacockery Voice API.
        </p>
      </div>
      <div className="grid grid-cols-1 gap-2 text-sm text-foreground">
        <div className="rounded bg-muted/40 p-3">
          <span className="font-semibold">Live transcript</span> while you talk,
          labelled Me and Them.
        </div>
        <div className="rounded bg-muted/40 p-3">
          <span className="font-semibold">Final transcript</span> after stop,
          with speaker labels.
        </div>
        <div className="rounded bg-muted/40 p-3">
          <span className="font-semibold">Summary</span> from a template,
          editable in place.
        </div>
      </div>
      <p className="border-t border-border pt-2 text-center text-xs text-muted-foreground">
        Internal build, forked from the open-source Meetily project (MIT).
      </p>
    </div>
  );
}
