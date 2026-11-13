"use client";

import { createContext, useContext } from "react";
import type { ReactNode } from "react";

interface ImportDialogContextType {
  openImportDialog: (filePath?: string | null) => void;
}

const ImportDialogContext = createContext<ImportDialogContextType | null>(null);

export const useImportDialog = () => {
  const ctx = useContext(ImportDialogContext);

  if (!ctx) {
    throw new Error("useImportDialog must be used within ImportDialogProvider");
  }

  return ctx;
};

export function ImportDialogProvider({
  children,
  onOpen,
}: {
  children: ReactNode;
  onOpen: (filePath?: string | null) => void;
}) {
  return (
    <ImportDialogContext.Provider value={{ openImportDialog: onOpen }}>
      {children}
    </ImportDialogContext.Provider>
  );
}
