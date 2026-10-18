import { commands, events, type DeviceSnapshot, type DeviceInfo } from "./bindings";

export const device = $state({
  snapshot: null as DeviceSnapshot | null,
  /** Connection panel opened from the device bar while the phone is ready. */
  panelOpen: false,
});

export async function startDevices() {
  await events.deviceEvent.listen((e) => {
    device.snapshot = e.payload;
  });
  device.snapshot = await commands.deviceSnapshot();
}

export function isReady(): boolean {
  return device.snapshot?.state.kind === "ready";
}

/** "Pixel 10 Pro"; adb's `model:` uses `_` for spaces. */
export function displayName(info: DeviceInfo | null, fallbackModel?: string | null): string {
  return info?.model || fallbackModel?.replaceAll("_", " ") || "";
}
