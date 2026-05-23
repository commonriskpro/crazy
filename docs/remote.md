# Remote collaboration / cryptographic identity

<!-- Implementation Status: fully implemented in crates/ail-remote. AgentIdentity, AgentKeypair, ObjectBundle, SignedContextSlice, RemoteChangeSet, and RemoteError all exist. Crypto primitives (AES-256-GCM, Argon2id, X25519) exist under feature = "crypto". -->

> Full extracted design. Related: [Coordinator](coordinator.md), [Context Server](context-server.md), [AI Change Language](change-language.md), [Storage](storage.md).

## Propósito

Cuando múltiples agentes colaboran de forma remota necesitan tres garantías:

```txt
1. Autenticidad  — saber qué agente produjo cada changeset o slice de contexto.
2. Integridad    — detectar cualquier modificación posterior a la firma.
3. Confidencialidad (opcional) — cifrar objetos para transporte punto a punto.
```

`ail-remote` provee los primitivos criptográficos para estas tres garantías sin acoplarse a ningún protocolo de transporte concreto.

### Crate contract

`ail-remote` es un crate hoja. Depende de `ail-storage` (para `ObjectId` y `CborCodec`) y de `ail-change` / `ail-context` (para tipos de dominio). No debe depender de `ail-coordinator`, `ail-verify`, `ail-runtime` ni `ail-compiler`.

## Identidad de agente

### AgentIdentity

```rust
pub struct AgentIdentity {
    pub public_key: [u8; 32],  // clave pública Ed25519 (32 bytes raw)
    pub label: Option<String>, // etiqueta legible, NO autenticada
}
```

`label` es informativo. Las decisiones de acceso deben basarse exclusivamente en `public_key`.

`AgentIdentity::verify_bytes(payload, sig)` reconstruye la `VerifyingKey` en cada llamada. La construcción de la clave es barata; no se cachea estado mutable.

### AgentKeypair

```rust
pub struct AgentKeypair { /* secret key never exposed */ }

impl AgentKeypair {
    pub fn generate() -> Self
    pub fn identity(&self) -> AgentIdentity
    pub fn sign_bytes(&self, payload: &[u8]) -> [u8; 64]
}
```

La clave privada nunca se expone por API pública. `sign_bytes` devuelve 64 bytes raw Ed25519 (no DER).

### Serialización CBOR

Las firmas son `[u8; 64]`. `ciborium` no soporta arrays fijos de más de 32 elementos vía derive, por lo que se usa un shim `sig_serde` que serializa la firma como CBOR byte string.

## ObjectBundle

Contenedor de objetos content-addressed para transferencia cross-boundary.

```rust
pub struct ObjectBundle {
    pub root: ObjectId,
    pub objects: BTreeMap<ObjectId, Vec<u8>>,
}
```

Cada entrada está indexada por el hash BLAKE3 de sus bytes. `verify_integrity()` re-deriva el `ObjectId` esperado de cada valor y lo compara contra la clave almacenada.

| Error | Condición |
|-------|-----------|
| `BundleError::RootNotFound` | La clave `root` no existe en `objects` |
| `BundleError::HashMismatch { object_id }` | Los bytes de una entrada no producen su clave declarada |

`BTreeMap` (no `HashMap`) garantiza orden determinístico en la serialización CBOR.

## Envelopes firmados

### SignedContextSlice

Envuelve un `ContextResponse` con una firma Ed25519.

```rust
pub struct SignedContextSlice {
    pub response: ContextResponse,
    pub signer: AgentIdentity,
    pub signature: [u8; 64],
}
```

**Payload de firma**: `CBOR([snapshot_id_bytes: [u8;32], context_hash: [u8;32], structured_cbor: Vec<u8>])`

Los tres campos cubren la identidad del snapshot, el hash del contexto, y la lista serializada de nodos. Cualquier modificación posterior invalida la firma.

```txt
SignedContextSlice::sign(response, &keypair)  →  Ok(slice)
slice.verify()  →  Ok(()) | Err(SignatureInvalid)
```

### RemoteChangeSet

Envuelve un `CanonicalChangeSet` con una firma Ed25519.

```rust
pub struct RemoteChangeSet {
    pub changeset: CanonicalChangeSet,
    pub agent: AgentIdentity,
    pub signature: [u8; 64],
}
```

**Payload de firma**: `CBOR([base_snapshot_id_bytes: [u8;8], ops_cbor: Vec<u8>])`

El `base_snapshot_id` se serializa como 8 bytes little-endian. Las ops se pre-codifican a CBOR antes de envolver en la tupla exterior, lo que evita problemas de serialización genérica y mantiene el determinismo.

```txt
RemoteChangeSet::sign(changeset, &keypair)  →  Ok(rcs)
rcs.verify_signature()  →  Ok(()) | Err(SignatureInvalid)
```

El `Coordinator` llama `verify_remote_submission(rcs)` que encadena verificación de firma → submit.

## RemoteError

```txt
RemoteError::SignatureInvalid
    La firma Ed25519 del envelope no pasó la verificación.
    El snapshot vivo no avanza.

RemoteError::CoordinatorFailed(reason)
    La firma era válida pero el coordinator devolvió Failed.
```

`RemoteError` está definido en `ail-remote` (no en `ail-coordinator`) para que el crate hoja pueda declarar el contrato sin depender del coordinator.

## Primitivos criptográficos (`feature = "crypto"`)

Compilados solo cuando se activa la feature `crypto`. Los tres crates operan enteramente en Rust seguro; el `deny(unsafe_code)` del workspace se aplica al código propio.

### AES-256-GCM

```rust
encrypt_aes256gcm(key: &[u8;32], nonce: &[u8;12], plaintext: &[u8]) → Result<Vec<u8>, CryptoError>
decrypt_aes256gcm(key: &[u8;32], nonce: &[u8;12], ciphertext: &[u8]) → Result<Vec<u8>, CryptoError>
```

El ciphertext incluye el tag GCM de 16 bytes al final. **El nonce debe ser único por par (clave, mensaje).** Reusar nonce es una falla de seguridad catastrófica.

### Argon2id

```rust
derive_key_argon2(password: &[u8], salt: &[u8;16]) → Result<[u8;32], CryptoError>
```

Parámetros fijos en mínimos OWASP interactivos: `m=65536` (64 MiB), `t=3`, `p=1`. Para contextos offline o de alta seguridad usar parámetros mayores. La sal debe ser única por credencial.

### X25519 ECDH

```rust
x25519_shared_secret(my_secret: &[u8;32], their_public: &[u8;32]) → [u8;32]
```

Devuelve el punto de Montgomery crudo. **Se debe pasar por un KDF** (p.ej. `derive_key_argon2` o HKDF) antes de usarlo como clave simétrica.

### CryptoError

| Variante | Condición |
|----------|-----------|
| `EncryptionFailed` | AEAD reportó error durante cifrado (prácticamente imposible para inputs válidos) |
| `DecryptionFailed` | Tag GCM no coincide (ciphertext alterado, clave incorrecta, o nonce incorrecto) |
| `KeyDerivationFailed` | Argon2id reportó error (combinación de parámetros inválida) |

### Notas de implementación

`RemoteExchangeRequest` / `RemoteExchangeResponse` definen el límite de servicio independiente del transporte para enviar changesets firmados e intercambiar bundles de objetos. El `Coordinator` actualmente acepta bundles enviados después de verificar su integridad y devuelve `BundleMissing` para pulls porque todavía no hay un store remoto durable de bundles conectado.

Referencias de código: `crates/ail-remote/src/identity.rs`, `crates/ail-remote/src/signing.rs`, `crates/ail-remote/src/bundle.rs`, `crates/ail-remote/src/exchange.rs`, `crates/ail-remote/src/crypto.rs`, `crates/ail-remote/src/error.rs`, `crates/ail-coordinator/src/coordinator.rs`.
