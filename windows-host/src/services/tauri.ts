import { invoke } from "@tauri-apps/api/core";

import type {
  AudioState,
  DiagnosticLogState,
  DiscoverySnapshot,
  LocalConfig,
  VirtualAudio,
} from "../types/roomwave";

export function getDiscoveryState(): Promise<DiscoverySnapshot> {
  return invoke<DiscoverySnapshot>("get_discovery_state");
}

export function refreshDiscovery(): Promise<void> {
  return invoke("refresh_discovery");
}

export function getAudioState(): Promise<AudioState> {
  return invoke<AudioState>("get_audio_state");
}

export function getVirtualAudioState(): Promise<VirtualAudio> {
  return invoke<VirtualAudio>("get_virtual_audio_state");
}

export function getDiagnosticLogState(): Promise<DiagnosticLogState> {
  return invoke<DiagnosticLogState>("get_diagnostic_log_state");
}

export function startDiagnosticLog(): Promise<void> {
  return invoke("start_diagnostic_log");
}

export function stopDiagnosticLog(): Promise<void> {
  return invoke("stop_diagnostic_log");
}

export function revealDiagnosticLog(): Promise<void> {
  return invoke("reveal_diagnostic_log");
}

export function connectDevice(deviceId: string): Promise<void> {
  return invoke("connect_device", {
    deviceId,
  });
}

export function disconnectAudio(deviceId: string | null): Promise<void> {
  return invoke("disconnect_audio", {
    deviceId,
  });
}

export function setChannel(deviceId: string, speaker: number | null): Promise<void> {
  return invoke("set_channel", {
    deviceId,
    speaker,
  });
}

export function testSpeaker(speaker: number): Promise<void> {
  return invoke("test_speaker", {
    speaker,
  });
}

export function setLocalOutput(config: LocalConfig): Promise<void> {
  return invoke("set_local_output", {
    config,
  });
}
