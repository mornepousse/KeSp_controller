// The browser's Web Serial port, and its way of opening and saving files.
// Works in Chrome and Edge, on https or localhost.

let port = null;
let reader = null;
let writer = null;
let pending = null;

export function serial_supported() {
  return !!navigator.serial;
}

// A port the person already allowed on an earlier visit.
export async function serial_granted() {
  if (!navigator.serial) return false;
  const ports = await navigator.serial.getPorts();
  if (ports.length === 0) return false;
  port = ports[0];
  return true;
}

// Must follow a click: the browser shows its own list of devices.
export async function serial_request() {
  port = await navigator.serial.requestPort({
    filters: [{ usbVendorId: 0xcafe }, { usbVendorId: 0x303a }],
  });
}

export async function serial_open() {
  await port.open({ baudRate: 115200 });
  writer = port.writable.getWriter();
  reader = port.readable.getReader();
  pending = null;
}

export async function serial_write(bytes) {
  await writer.write(bytes);
}

// What arrives within `ms`, empty if nothing does. A read still waiting is kept
// for next time: cancelling it could lose bytes.
export async function serial_read(ms) {
  if (!pending) pending = reader.read();
  const timeout = new Promise((resolve) => setTimeout(() => resolve(null), ms));
  const result = await Promise.race([pending, timeout]);
  if (result === null) return new Uint8Array(0);
  pending = null;
  if (result.done) throw new Error("port closed");
  return result.value;
}

export async function serial_close() {
  try {
    if (reader) {
      await reader.cancel();
      reader.releaseLock();
    }
    if (writer) {
      writer.releaseLock();
    }
    if (port) await port.close();
  } catch (e) {
    console.warn(e);
  }
  port = reader = writer = pending = null;
}

export function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// [name, text] of a file the person picks. Never resolves if they cancel.
export function pick_text(accept) {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.onchange = async () => {
      const file = input.files[0];
      if (!file) return;
      try {
        resolve([file.name, await file.text()]);
      } catch (e) {
        reject(e);
      }
    };
    input.click();
  });
}

export function download(name, text) {
  const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
