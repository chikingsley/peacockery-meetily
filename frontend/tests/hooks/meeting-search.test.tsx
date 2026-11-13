import { afterEach, expect, test } from "bun:test";

import { act, create } from "react-test-renderer";
import type { ReactTestRenderer } from "react-test-renderer";

import { useMeetingSearch } from "../../src/hooks/useMeetingSearch";
import type { TranscriptSearchResult } from "../../src/types/voice";

let view: ReactTestRenderer | undefined;
let state: ReturnType<typeof useMeetingSearch>;
const pending = new Map<string, (rows: TranscriptSearchResult[]) => void>();

const search = (query: string) =>
  new Promise<TranscriptSearchResult[]>((resolve) => {
    pending.set(query, resolve);
  });

function Consumer() {
  state = useMeetingSearch(search, 5);

  return null;
}

const tick = () =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, 15);
  });

const hit = (id: string): TranscriptSearchResult => ({
  id,
  title: id,
  matchContext: id,
  timestamp: null,
});

afterEach(async () => {
  await act(() => view?.unmount());
  view = undefined;
  pending.clear();
});

test("search debounces typing and only the newest result can settle loading", async () => {
  await act(() => {
    view = create(<Consumer />);
  });
  await act(async () => {
    await state.searchTranscripts("old");
    await state.searchTranscripts("first");
  });
  await act(tick);
  expect(pending.has("old")).toBe(false);
  expect(pending.has("first")).toBe(true);
  await act(async () => {
    await state.searchTranscripts("second");
  });
  await act(tick);
  await act(() => {
    pending.get("first")?.([hit("stale")]);
  });
  expect(state.searchResults).toEqual([]);
  expect(state.isSearching).toBe(true);
  await act(() => {
    pending.get("second")?.([hit("current")]);
  });
  expect(state.searchResults.map((row) => row.id)).toEqual(["current"]);
  expect(state.isSearching).toBe(false);
});

test("clearing a search invalidates an in-flight response", async () => {
  await act(() => {
    view = create(<Consumer />);
  });
  await act(async () => {
    await state.searchTranscripts("pending");
  });
  await act(tick);
  await act(async () => {
    await state.searchTranscripts("");
  });
  await act(() => {
    pending.get("pending")?.([hit("stale")]);
  });
  expect(state.searchResults).toEqual([]);
  expect(state.isSearching).toBe(false);
});
