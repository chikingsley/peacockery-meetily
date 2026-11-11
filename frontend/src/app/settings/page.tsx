'use client';

import React, { Suspense, useLayoutEffect, useRef, useState } from 'react';
import { ArrowLeft, AudioLines, Mic, Settings2 } from 'lucide-react';
import { useRouter, useSearchParams } from 'next/navigation';
import { motion } from 'framer-motion';
import { VoiceSettingsTab } from '@/components/settings/VoiceSettingsTab';
import { RecordingTab } from '@/components/settings/RecordingTab';
import { GeneralTab } from '@/components/settings/GeneralTab';
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs';

const TABS = [
  { value: 'voice', label: 'Peacockery Voice', icon: AudioLines },
  { value: 'recording', label: 'Recording', icon: Mic },
  { value: 'general', label: 'General', icon: Settings2 },
] as const;

type TabValue = typeof TABS[number]['value'];

function SettingsContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const initialTab = TABS.some(t => t.value === searchParams.get('tab')) ? searchParams.get('tab') as TabValue : 'voice';
  const [activeTab, setActiveTab] = useState<TabValue>(initialTab);
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [underlineStyle, setUnderlineStyle] = useState({ left: 0, width: 0 });

  useLayoutEffect(() => {
    const el = tabRefs.current[TABS.findIndex(tab => tab.value === activeTab)];
    if (el) setUnderlineStyle({ left: el.offsetLeft, width: el.offsetWidth });
  }, [activeTab]);

  return (
    <div className="h-screen bg-gray-50 flex flex-col">
      <div className="sticky top-0 z-10 bg-gray-50 border-b border-gray-200">
        <div className="max-w-3xl mx-auto px-8 py-6">
          <div className="flex items-center gap-4">
            <button
              onClick={() => router.back()}
              className="flex items-center gap-2 text-gray-600 hover:text-gray-900 transition-colors"
            >
              <ArrowLeft className="w-5 h-5" />
              <span>Back</span>
            </button>
            <h1 className="text-3xl font-bold">Settings</h1>
          </div>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto">
        <div className="max-w-3xl mx-auto p-8 pt-6">
          <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as TabValue)}>
            <TabsList className="bg-transparent relative rounded-none border-b border-gray-200 p-0 h-auto">
              {TABS.map((tab, index) => {
                const Icon = tab.icon;
                return (
                  <TabsTrigger
                    key={tab.value}
                    value={tab.value}
                    ref={el => { tabRefs.current[index] = el }}
                    className="flex items-center gap-2 px-6 py-4 bg-transparent rounded-none border-0 data-[state=active]:bg-transparent data-[state=active]:text-blue-600 data-[state=active]:shadow-none text-gray-600 hover:text-gray-900 relative z-10"
                  >
                    <Icon className="w-4 h-4" />
                    {tab.label}
                  </TabsTrigger>
                );
              })}
              <motion.div
                className="absolute bottom-0 z-20 h-0.5 bg-blue-600"
                layoutId="underline"
                style={{ left: underlineStyle.left, width: underlineStyle.width }}
                transition={{ type: 'spring', stiffness: 400, damping: 40 }}
              />
            </TabsList>

            <TabsContent value="voice" className="mt-6">
              <VoiceSettingsTab />
            </TabsContent>
            <TabsContent value="recording" className="mt-6">
              <RecordingTab />
            </TabsContent>
            <TabsContent value="general" className="mt-6">
              <GeneralTab />
            </TabsContent>
          </Tabs>
        </div>
      </div>
    </div>
  );
}

export default function SettingsPage() {
  return (
    <Suspense fallback={null}>
      <SettingsContent />
    </Suspense>
  );
}
