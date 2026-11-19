/**
 * Generate a UUID v4. `crypto.randomUUID` is only exposed in secure contexts,
 * so plain-HTTP LAN access falls back to `crypto.getRandomValues` (or Math.random).
 */
export function randomUuid(): string {
  const webCrypto = globalThis.crypto;
  if (typeof webCrypto?.randomUUID === "function") {
    return webCrypto.randomUUID();
  }

  const bytes = new Uint8Array(16);
  if (webCrypto && typeof webCrypto.getRandomValues === "function") {
    webCrypto.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }

  const versionByte = bytes[6];
  if (versionByte !== undefined) {
    bytes[6] = (versionByte & 0x0f) | 0x40;
  }
  const variantByte = bytes[8];
  if (variantByte !== undefined) {
    bytes[8] = (variantByte & 0x3f) | 0x80;
  }

  const hex = Array.from(bytes, (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20),
  ].join("-");
}
