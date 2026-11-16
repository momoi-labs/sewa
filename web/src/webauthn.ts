import type { ApprovalOptions, Assertion, PasskeyOptions, PasskeyRegistration } from './api';

export function encode(bytes: ArrayBuffer | Uint8Array): string {
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let text = '';
  for (const byte of view) text += String.fromCharCode(byte);
  return btoa(text).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function decode(text: string): Uint8Array<ArrayBuffer> {
  const base64 = text.replace(/-/g, '+').replace(/_/g, '/');
  const binary = atob(base64 + '='.repeat((4 - (base64.length % 4)) % 4));
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

export class PasskeyFailure extends Error {
  readonly reason: string;

  constructor(reason: string) {
    super(`Passkey ceremony failed: ${reason}`);
    this.reason = reason;
  }
}

function failure(error: unknown): PasskeyFailure {
  if (error instanceof PasskeyFailure) return error;
  const name = error instanceof DOMException ? error.name : '';
  if (name === 'NotAllowedError' || name === 'AbortError') return new PasskeyFailure('cancelled');
  if (name === 'InvalidStateError') return new PasskeyFailure('exists');
  if (name === 'NotSupportedError' || name === 'SecurityError') return new PasskeyFailure('unsupported');
  return new PasskeyFailure('failed');
}

function supported() {
  return typeof window !== 'undefined' && 'PublicKeyCredential' in window && !!navigator.credentials;
}

export async function createPasskey(options: PasskeyOptions, name: string): Promise<PasskeyRegistration> {
  if (!supported()) throw new PasskeyFailure('unsupported');
  try {
    const credential = (await navigator.credentials.create({
      publicKey: {
        challenge: decode(options.challenge),
        rp: options.rp,
        user: { id: decode(options.user.id), name: options.user.name, displayName: options.user.name },
        pubKeyCredParams: [{ type: 'public-key', alg: options.algorithm }],
        excludeCredentials: options.exclude_credentials.map((id) => ({ type: 'public-key', id: decode(id) })),
        authenticatorSelection: { residentKey: 'preferred', userVerification: 'required' },
        attestation: 'none',
      },
    })) as PublicKeyCredential | null;
    if (!credential) throw new PasskeyFailure('cancelled');
    const response = credential.response as AuthenticatorAttestationResponse;
    const publicKey = response.getPublicKey();
    if (!publicKey || response.getPublicKeyAlgorithm() !== options.algorithm) {
      throw new PasskeyFailure('unsupported');
    }
    return {
      name,
      credential_id: encode(credential.rawId),
      public_key: encode(publicKey),
      algorithm: response.getPublicKeyAlgorithm(),
      authenticator_data: encode(response.getAuthenticatorData()),
      client_data_json: encode(response.clientDataJSON),
    };
  } catch (error) {
    throw failure(error);
  }
}

export async function signApproval(options: ApprovalOptions): Promise<Assertion> {
  if (!supported()) throw new PasskeyFailure('unsupported');
  try {
    const credential = (await navigator.credentials.get({
      publicKey: {
        challenge: decode(options.challenge),
        rpId: options.rp_id,
        allowCredentials: options.allow_credentials.map((id) => ({ type: 'public-key', id: decode(id) })),
        userVerification: 'required',
      },
    })) as PublicKeyCredential | null;
    if (!credential) throw new PasskeyFailure('cancelled');
    const response = credential.response as AuthenticatorAssertionResponse;
    return {
      credential_id: encode(credential.rawId),
      authenticator_data: encode(response.authenticatorData),
      client_data_json: encode(response.clientDataJSON),
      signature: encode(response.signature),
    };
  } catch (error) {
    throw failure(error);
  }
}
