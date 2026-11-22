import { useEffect, useState } from "react";

function nextMidnight(now: Date): number {
  const next = new Date(now);
  next.setHours(24, 0, 0, 0);
  return next.getTime();
}

/**
 * The current moment, refreshed once at every local midnight, so "Today" becomes "Yesterday"
 * while the app runs. A local timer, not a request to the core.
 */
export function useToday(): Date {
  const [today, setToday] = useState(() => new Date());
  useEffect(() => {
    // A timer can fire a little early or, after sleep, late; it always re-arms for the next day.
    const delay = Math.max(1000, nextMidnight(today) - Date.now() + 1000);
    const timer = window.setTimeout(() => {
      setToday(new Date());
    }, delay);
    return () => {
      window.clearTimeout(timer);
    };
  }, [today]);
  return today;
}
