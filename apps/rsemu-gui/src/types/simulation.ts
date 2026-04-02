export interface SimStatusPayload {
  steps: number;
  running: boolean;
  error?: string;
}

export interface LedChangedPayload {
  id: string;
  on: boolean;
}

export interface DisplayFramePayload {
  width: number;
  height: number;
  /** ARGB pixels, base64-encoded (4 bytes per pixel, LE) */
  data: string;
}

export interface UartOutputPayload {
  peripheral: string;
  byte: number;
}
