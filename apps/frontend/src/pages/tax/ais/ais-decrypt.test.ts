import { describe, expect, it } from "vitest";
import { candidatePasswords, decryptAis, isEncryptedAis } from "./ais-decrypt";

function hex(bytes: Uint8Array): string {
  return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** Encrypts the way the AIS utility does, to test the reverse. */
async function encrypt(plain: string, password: string): Promise<string> {
  const subtle = globalThis.crypto.subtle;
  const iv = globalThis.crypto.getRandomValues(new Uint8Array(16));
  const salt = globalThis.crypto.getRandomValues(new Uint8Array(16));
  const base = await subtle.importKey("raw", new TextEncoder().encode(password), "PBKDF2", false, [
    "deriveKey",
  ]);
  const key = await subtle.deriveKey(
    { name: "PBKDF2", hash: "SHA-256", salt, iterations: 1000 },
    base,
    { name: "AES-CBC", length: 256 },
    false,
    ["encrypt"],
  );
  const data = new Uint8Array(
    await subtle.encrypt({ name: "AES-CBC", iv }, key, new TextEncoder().encode(plain)),
  );
  return hex(iv) + hex(salt) + btoa(String.fromCharCode(...data));
}

describe("AIS decryption", () => {
  const plain = JSON.stringify({ hello: "AIS" });

  it("opens the export with the PAN and date of birth", async () => {
    const [password] = candidatePasswords("ABCDE1234F", "15/07/1985");
    expect(password).toBe("abcde1234fGQ39%*g15071985");
    const file = await encrypt(plain, password);
    expect(isEncryptedAis(file)).toBe(true);
    expect(isEncryptedAis(plain)).toBe(false);
    await expect(decryptAis(file, "ABCDE1234F", "15-07-1985")).resolves.toBe(plain);
  });

  it("says so when the PAN or date of birth is wrong", async () => {
    const file = await encrypt(plain, candidatePasswords("ABCDE1234F", "15071985")[0]);
    await expect(decryptAis(file, "ABCDE1234F", "16071985")).rejects.toThrow(/PAN and date/);
  });
});
