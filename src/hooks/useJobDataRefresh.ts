import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

/** Refresh stored data after writes, including writes from another app process. */
export function useJobDataRefresh(refresh: () => Promise<unknown>, intervalMs = 3000) {
  const callback = useRef(refresh);
  useEffect(() => { callback.current = refresh; }, [refresh]);
  useEffect(() => {
    let disposed = false;
    let pending = false;
    let dirty = false;
    const run = async () => {
      if (disposed || document.hidden) return;
      if (pending) { dirty = true; return; }
      pending = true;
      try { await callback.current(); } catch { /* Existing page error handling remains responsible. */ }
      finally {
        pending = false;
        if (dirty && !disposed) { dirty = false; void run(); }
      }
    };
    const wake = () => { void run(); };
    const unsubscribe = listen("job-data-changed", wake).catch(() => null);
    const timer = window.setInterval(wake, intervalMs);
    window.addEventListener("focus", wake);
    document.addEventListener("visibilitychange", wake);
    return () => {
      disposed = true;
      window.clearInterval(timer);
      window.removeEventListener("focus", wake);
      document.removeEventListener("visibilitychange", wake);
      void unsubscribe.then(unlisten => unlisten?.());
    };
  }, [intervalMs]);
}
