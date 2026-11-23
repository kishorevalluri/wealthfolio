/**
 * Opens the encrypted JSON the income-tax portal gives for the AIS offline
 * utility. The file is text: 32 hex characters of IV, 32 of salt, then the
 * AES-CBC ciphertext in Base64. The key is PBKDF2-SHA256 (1,000 rounds) of a
 * password built from the PAN and date of birth.
 *
 * The layout is not published; it follows what the utility does today and may
 * change with a new utility version. Everything happens in this window: the
 * PAN and date of birth are not stored.
 */

const SALT_AND_IV = 64;
/** Fixed text the utility puts between the PAN and the date of birth. */
const UTILITY_MIDDLE = "GQ39%*g";

/** Whether `text` looks like the encrypted export rather than plain JSON. */
export function isEncryptedAis(text: string): boolean {
  let raw = text.trim();
  if (raw.startsWith('"') && raw.endsWith('"')) raw = raw.slice(1, -1);
  return /^[0-9a-f]{64}[A-Za-z0-9+/=\s]+$/i.test(raw.slice(0, 200)) && raw.length > SALT_AND_IV;
}

function hexBytes(hex: string): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

function base64Bytes(text: string): Uint8Array<ArrayBuffer> {
  const binary = atob(text.replace(/\s/g, ""));
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

/** "15/07/1985", "15-07-1985" or "15071985" → "15071985". */
export function dobDigits(dob: string): string {
  const digits = dob.replace(/\D/g, "");
  if (digits.length !== 8) throw new Error("Enter the date of birth as DD/MM/YYYY.");
  return digits;
}

/** The passwords the utility has used, most likely first. */
export function candidatePasswords(pan: string, dob: string): string[] {
  const p = pan.trim();
  const d = dobDigits(dob);
  return [
    `${p.toLowerCase()}${UTILITY_MIDDLE}${d}`,
    `${p.toLowerCase()}${d}`,
    `${p.toUpperCase()}${d}`,
  ];
}

async function decryptWith(
  password: string,
  iv: Uint8Array<ArrayBuffer>,
  salt: Uint8Array<ArrayBuffer>,
  data: Uint8Array<ArrayBuffer>,
): Promise<string> {
  const subtle = globalThis.crypto.subtle;
  const base = await subtle.importKey("raw", new TextEncoder().encode(password), "PBKDF2", false, [
    "deriveKey",
  ]);
  const key = await subtle.deriveKey(
    { name: "PBKDF2", hash: "SHA-256", salt, iterations: 1000 },
    base,
    { name: "AES-CBC", length: 256 },
    false,
    ["decrypt"],
  );
  const plain = await subtle.decrypt({ name: "AES-CBC", iv }, key, data);
  return new TextDecoder("utf-8", { fatal: true }).decode(plain);
}

/** Decrypts the export with the PAN and date of birth, or throws. */
export async function decryptAis(text: string, pan: string, dob: string): Promise<string> {
  let raw = text.trim();
  if (raw.startsWith('"') && raw.endsWith('"')) raw = JSON.parse(raw) as string;
  const iv = hexBytes(raw.slice(0, 32));
  const salt = hexBytes(raw.slice(32, SALT_AND_IV));
  const data = base64Bytes(raw.slice(SALT_AND_IV));
  for (const password of candidatePasswords(pan, dob)) {
    try {
      const plain = await decryptWith(password, iv, salt, data);
      JSON.parse(plain);
      return plain;
    } catch {
      // Wrong password: AES-CBC fails on the padding or the text is not JSON.
    }
  }
  throw new Error("Could not open the AIS file. Check the PAN and date of birth.");
}
