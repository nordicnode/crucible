// Minimal WebSocket wrapper with typed callbacks.

import type { ClientMsg, ServerMsg } from "./types";

export class Net {
  private ws: WebSocket | null = null;
  private queue: ClientMsg[] = [];
  private onMessage: ((msg: ServerMsg) => void) | null = null;
  private onClose: (() => void) | null = null;

  connect(onMessage: (msg: ServerMsg) => void, onClose: () => void): void {
    this.onMessage = onMessage;
    this.onClose = onClose;
    const proto = location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(`${proto}://${location.host}/ws`);
    this.ws = ws;
    ws.onopen = () => {
      for (const m of this.queue) ws.send(JSON.stringify(m));
      this.queue = [];
    };
    ws.onmessage = (ev) => {
      const msg = JSON.parse(ev.data as string) as ServerMsg;
      this.onMessage?.(msg);
    };
    ws.onclose = () => this.onClose?.();
    ws.onerror = () => this.onClose?.();
  }

  send(msg: ClientMsg): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    } else {
      this.queue.push(msg);
    }
  }

  close(): void {
    this.ws?.close();
  }
}
