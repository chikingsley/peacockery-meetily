interface EmptyStateSummaryProps {
  error?: string | null;
}

export function EmptyStateSummary({ error = null }: EmptyStateSummaryProps) {
  return (
    <div className="mx-auto max-w-sm space-y-3 px-6 py-8 text-sm text-muted-foreground">
      <p>
        Choose Generate summary above to create notes and action items from this
        transcript.
      </p>
      {error && (
        <p
          role="alert"
          className="rounded-md bg-destructive/10 p-3 text-destructive"
        >
          {error}
        </p>
      )}
    </div>
  );
}
