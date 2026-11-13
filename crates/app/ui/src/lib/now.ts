// The time, kept current for what says how long ago something was.

import { useEffect, useState } from "react";

/** Seconds since 1970, UTC, updated every `every` milliseconds while the component is shown. */
export function useNow(every = 30_000): number {
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now() / 1000), every);
    return () => clearInterval(timer);
  }, [every]);
  return now;
}
