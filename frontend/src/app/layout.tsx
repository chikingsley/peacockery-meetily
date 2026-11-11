'use client'

import './globals.css'
import "sonner/dist/styles.css"
import { Source_Sans_3 } from 'next/font/google'
import { useState, useEffect, useCallback } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { Toaster, toast } from 'sonner'
import Sidebar from '@/components/Sidebar'
import { SidebarProvider } from '@/components/Sidebar/SidebarProvider'
import MainContent from '@/components/MainContent'
import { TooltipProvider } from '@/components/ui/tooltip'
import { RecordingStateProvider } from '@/contexts/RecordingStateContext'
import { TranscriptProvider } from '@/contexts/TranscriptContext'
import { ConfigProvider } from '@/contexts/ConfigContext'
import { RecordingStopProvider } from '@/contexts/RecordingStopContext'
import { ImportDialogProvider } from '@/contexts/ImportDialogContext'
import { ImportAudioDialog, ImportDropOverlay } from '@/components/ImportAudio'
import { isAudioExtension, getAudioFormatsDisplayList } from '@/constants/audioFormats'


const sourceSans3 = Source_Sans_3({
  subsets: ['latin'],
  weight: ['400', '500', '600', '700'],
  variable: '--font-source-sans-3',
})

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  const [showDropOverlay, setShowDropOverlay] = useState(false)
  const [showImportDialog, setShowImportDialog] = useState(false)
  const [importFilePath, setImportFilePath] = useState<string | null>(null)

  // Disable the browser context menu in production builds.
  useEffect(() => {
    if (process.env.NODE_ENV !== 'production') return
    const handleContextMenu = (e: MouseEvent) => e.preventDefault()
    document.addEventListener('contextmenu', handleContextMenu)
    return () => document.removeEventListener('contextmenu', handleContextMenu)
  }, [])



  const openImportDialog = useCallback((filePath?: string | null) => {
    setImportFilePath(filePath ?? null)
    setShowImportDialog(true)
  }, [])

  const handleFileDrop = useCallback((paths: string[]) => {
    const audioFile = paths.find(p => {
      const ext = p.split('.').pop()?.toLowerCase()
      return !!ext && isAudioExtension(ext)
    })
    if (audioFile) {
      openImportDialog(audioFile)
    } else if (paths.length > 0) {
      toast.error('Please drop an audio file', {
        description: `Supported formats: ${getAudioFormatsDisplayList()}`,
      })
    }
  }, [openImportDialog])

  // Drag-and-drop audio import.
  useEffect(() => {
    let cancelled = false
    const unlisteners: UnlistenFn[] = []
    const keep = (fn: UnlistenFn) => {
      if (cancelled) fn()
      else unlisteners.push(fn)
    }
    ;(async () => {
      keep(await listen('tauri://drag-enter', () => setShowDropOverlay(true)))
      keep(await listen('tauri://drag-leave', () => setShowDropOverlay(false)))
      keep(await listen<{ paths: string[] }>('tauri://drag-drop', (event) => {
        setShowDropOverlay(false)
        handleFileDrop(event.payload.paths)
      }))
    })()
    return () => {
      cancelled = true
      unlisteners.forEach(fn => fn())
    }
  }, [handleFileDrop])

  const handleImportDialogChange = useCallback((open: boolean) => {
    setShowImportDialog(open)
    if (!open) setImportFilePath(null)
  }, [])

  return (
    <html lang="en">
      <head>
        <title>Peacockery Meetily</title>
      </head>
      <body className={`${sourceSans3.variable} font-sans antialiased`}>
        <RecordingStateProvider>
          <TranscriptProvider>
            <ConfigProvider>
              <SidebarProvider>
                <RecordingStopProvider>
                  <TooltipProvider>
                    <ImportDialogProvider onOpen={openImportDialog}>
                      <div className="flex">
                        <Sidebar />
                        <MainContent>{children}</MainContent>
                      </div>
                      <ImportDropOverlay visible={showDropOverlay} />
                      <ImportAudioDialog
                        open={showImportDialog}
                        onOpenChange={handleImportDialogChange}
                        preselectedFile={importFilePath}
                      />
                    </ImportDialogProvider>
                  </TooltipProvider>
                </RecordingStopProvider>
              </SidebarProvider>
            </ConfigProvider>
          </TranscriptProvider>
        </RecordingStateProvider>
        <Toaster position="bottom-center" richColors closeButton />
      </body>
    </html>
  )
}
