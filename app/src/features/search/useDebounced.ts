import { useEffect, useState } from "react";

/** `value`, but only after it stayed the same for `delay` ms; at once when `delay` is 0. */
export function useDebounced<T>(value: T, delay: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    if (delay <= 0) return undefined;
    const timer = window.setTimeout(() => {
      setSettled(value);
    }, delay);
    return () => {
      window.clearTimeout(timer);
    };
  }, [value, delay]);
  return delay <= 0 ? value : settled;
}
