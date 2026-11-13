"use client";

import { AudioLines, Mic, Settings2 } from "lucide-react";
import { useSearchParams } from "next/navigation";
import { Suspense, useState } from "react";

import { GeneralTab } from "@/components/settings/GeneralTab";
import { RecordingTab } from "@/components/settings/RecordingTab";
import { VoiceSettingsTab } from "@/components/settings/VoiceSettingsTab";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";

function SettingsContent() {
  const params = useSearchParams();
  const initial = params.get("tab");

  const [tab, setTab] = useState(
    initial === "recording" || initial === "general" ? initial : "voice"
  );

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl px-5 py-5">
        <Tabs value={tab} onValueChange={setTab}>
          <TabsList className="mb-4 w-full">
            <TabsTrigger value="voice">
              <AudioLines />
              Voice
            </TabsTrigger>
            <TabsTrigger value="recording">
              <Mic />
              Audio
            </TabsTrigger>
            <TabsTrigger value="general">
              <Settings2 />
              General
            </TabsTrigger>
          </TabsList>
          <TabsContent value="voice" className="mt-0">
            <VoiceSettingsTab />
          </TabsContent>
          <TabsContent value="recording" className="mt-0">
            <RecordingTab />
          </TabsContent>
          <TabsContent value="general" className="mt-0">
            <GeneralTab />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}

export default function SettingsPage() {
  return (
    <Suspense
      fallback={
        <div className="p-6 text-muted-foreground">Loading settings…</div>
      }
    >
      <SettingsContent />
    </Suspense>
  );
}
