import React from "react";
import { ReceiverHealthMonitor } from "../lib/diagnosticsHealth";
import type { ReceiverHealth } from "../lib/diagnosticsHealth";
import type { AudioState } from "../types/roomwave";

// Mounted with App, so closing Diagnostics does not discard the observation window.
export function useReceiverHealth(audio: AudioState): ReceiverHealth[] {
  const monitor = React.useRef(new ReceiverHealthMonitor());
  const [health, setHealth] = React.useState<ReceiverHealth[]>([]);
  React.useEffect(() => {
    setHealth(monitor.current.observe(audio, performance.now()));
  }, [audio]);
  React.useEffect(() => {
    // Expire stale results even if host polling fails and no new AudioState arrives.
    const timer = setInterval(() => setHealth(monitor.current.summarize(performance.now())), 1000);
    return () => clearInterval(timer);
  }, []);
  return health;
}
