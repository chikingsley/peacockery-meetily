"use client";

import { FolderOpen, HardDrive } from "lucide-react";
import { useTheme } from "next-themes";
import { toast } from "sonner";

import { About } from "@/components/About";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { voiceService, errorMessage } from "@/services/voiceService";

import { useRecordingPreferences } from "./RecordingTab";
import { SettingsSection } from "./SettingsParts";

/** Settings → General: where recordings are saved. */
export function GeneralTab() {
  const { theme, setTheme } = useTheme();
  const { preferences } = useRecordingPreferences();

  return (
    <div className="space-y-6">
      <SettingsSection
        title="Appearance"
        description="Choose a theme for your workspace."
      >
        <Label htmlFor="appearance">Theme</Label>
        <Select value={theme ?? "system"} onValueChange={setTheme}>
          <SelectTrigger id="appearance">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="system">System</SelectItem>
            <SelectItem value="light">Light</SelectItem>
            <SelectItem value="dark">Dark</SelectItem>
          </SelectContent>
        </Select>
      </SettingsSection>
      <SettingsSection
        title="Recordings folder"
        icon={<HardDrive className="h-5 w-5" />}
        description="Every recording is always saved, one folder per meeting with its audio.mp4 and transcripts."
      >
        <div className="rounded-md border border-border bg-muted/40 p-3 font-mono text-sm break-all text-foreground">
          {preferences ? preferences.save_folder || "Default folder" : "…"}
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() =>
            voiceService.openRecordingsFolder().catch((error) =>
              toast.error("Failed to open the folder", {
                description: errorMessage(error),
              })
            )
          }
        >
          <FolderOpen className="mr-2 h-4 w-4" />
          Open folder
        </Button>
      </SettingsSection>
      <About />
    </div>
  );
}
