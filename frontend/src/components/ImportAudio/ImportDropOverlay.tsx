import { Upload } from "lucide-react";
import React from "react";

import { getAudioFormatsDisplayList } from "@/constants/audioFormats";

interface ImportDropOverlayProps {
  visible: boolean;
}

export function ImportDropOverlay({ visible }: ImportDropOverlayProps) {
  if (!visible) {
    return null;
  }

  return (
    <div className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-foreground/50 backdrop-blur-sm transition-opacity duration-200">
      <div className="scale-100 transform rounded-2xl border-2 border-dashed border-info/30 bg-info/50 p-12 text-center shadow-2xl transition-transform">
        <Upload className="mx-auto mb-4 h-16 w-16 text-info" />
        <p className="text-xl font-medium text-primary-foreground">
          Drop audio file to import
        </p>
        <p className="mt-2 text-sm text-info">{getAudioFormatsDisplayList()}</p>
      </div>
    </div>
  );
}
