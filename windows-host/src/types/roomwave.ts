export interface Device {
    deviceId: string;
    deviceName: string;
    ipAddress: string;
    port: number;
    protocolVersion: number;
}

export interface DiscoverySnapshot {
    devices: Device[];
    discovering: boolean;
    error: string | null;
}

export interface ReceiverState {
    deviceId: string;
    deviceName: string;
    status: string;
    packetsSent: number;

    receiverPackets: number;
    receiverLost: number;
    latencyMs: number | null;
    rttMs: number | null;

    syncErrorMs: number | null;
    syncStatus: string;
    error: string | null;

    stages: Record<string, unknown> | null;
    transport?: Record<string, unknown>;
}

export interface Speaker {
    mask: number;
    code: string;
    name: string;
    index: number;
}

export interface LocalConfig {
    enabled: boolean;
    sourceId: string | null;
    outputId: string | null;
    speakers: number[];
}

export interface AudioState {
    streamMode?: "quality" | "latency";
    modeRevision?: number;
    hostMetrics?: Record<string, unknown>;

    outputName: string | null;
    peak: number;
    targetDelayMs: number;

    receivers: ReceiverState[];

    layout: {
        name: string;
        channelCount: number;
        channelMask: number;
        channels: Speaker[];
        error: string | null;
    };

    assignments: Record<string, number>;
    captureError: string | null;
    testing: number | null;

    localConfig: LocalConfig;

    windowsOutputs: {
        id: string;
        name: string;
    }[];

    localOutput: {
        status: string;
        error: string | null;
        syncErrorMs: number | null;
        latencyMs: number | null;
        outputMs: number | null;
        unavailable: number[];
    };
}

export interface VirtualAudio {
    endpointId: string | null;
    previousOutputId: string | null;
    active: boolean;
    error: string | null;
}

export interface DiagnosticLogState {
    path: string | null;
    recording: boolean;
    samples: number;
    error: string | null;
    endsAtUnixMs: number | null;
}

export type Action = (
    key: string,
    run: () => Promise<unknown>,
) => Promise<void>;
