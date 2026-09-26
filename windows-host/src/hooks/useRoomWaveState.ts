import React from "react";
import { invoke } from "@tauri-apps/api/core";

import type {
  Action,
  AudioState,
  DiagnosticLogState,
  DiscoverySnapshot,
  VirtualAudio,
} from "../types/roomwave";

const POLL_INTERVAL_MS = 500;

const initialAudioState: AudioState = {
  outputName: null,
  peak: 0,
  targetDelayMs: 80,

  receivers: [],

  layout: {
    name: "…",
    channelCount: 0,
    channelMask: 0,
    channels: [],
    error: null,
  },

  assignments: {},
  captureError: null,
  testing: null,

  localConfig: {
    enabled: false,
    sourceId: null,
    outputId: null,
    speakers: [],
  },

  windowsOutputs: [],

  localOutput: {
    status: "disabled",
    error: null,
    syncErrorMs: null,
    latencyMs: null,
    outputMs: null,
    unavailable: [],
  },
};

export function useRoomWaveState() {
  const [virtualAudio, setVirtualAudio] = React.useState<VirtualAudio | null>(null);

  const [logState, setLogState] = React.useState<DiagnosticLogState | null>(null);

  const [snapshot, setSnapshot] = React.useState<DiscoverySnapshot>({
    devices: [],
    discovering: false,
    error: null,
  });

  const [audio, setAudio] = React.useState<AudioState>(initialAudioState);

  const [loaded, setLoaded] = React.useState(false);

  const [error, setError] = React.useState<string | null>(null);

  const [busy, setBusy] = React.useState<Set<string>>(new Set());

  const pending = React.useRef(new Set<string>());

  React.useEffect(() => {
    let disposed = false;

    let timer: ReturnType<typeof setTimeout>;

    async function update() {
      try {
        const [devices, state, log, virtualState] = await Promise.all([
          invoke<DiscoverySnapshot>("get_discovery_state"),
          invoke<AudioState>("get_audio_state"),
          invoke<DiagnosticLogState>("get_diagnostic_log_state"),
          invoke<VirtualAudio>("get_virtual_audio_state"),
        ]);

        if (!disposed) {
          setSnapshot(devices);
          setAudio(state);
          setLogState(log);
          setVirtualAudio(virtualState);
          setLoaded(true);
        }
      } catch (caughtError) {
        if (!disposed) {
          setError(String(caughtError));
        }
      } finally {
        if (!disposed) {
          timer = setTimeout(update, POLL_INTERVAL_MS);
        }
      }
    }

    void update();

    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, []);

  const action: Action = async (key, run) => {
    if (pending.current.has(key)) {
      return;
    }

    pending.current.add(key);
    setBusy(new Set(pending.current));

    try {
      await run();

      setAudio(await invoke<AudioState>("get_audio_state"));
      setError(null);
    } catch (caughtError) {
      setError(String(caughtError));
    } finally {
      pending.current.delete(key);
      setBusy(new Set(pending.current));
    }
  };

  return {
    virtualAudio,
    logState,
    setLogState,
    snapshot,
    audio,
    loaded,
    error,
    busy,
    action,
  };
}
