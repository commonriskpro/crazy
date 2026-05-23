# Remote collaboration / cryptographic identity

<!-- Status: Implemented subset. `ail-remote` contains identities, signer policy, object bundles, file bundle storage, signed context slices, remote ChangeSets, and optional crypto primitives. Durable network transport and remote discovery remain target design. -->

> Target design. Current implementation scope is called out in the status note. Related: [Coordinator](coordinator.md), [Context Server](context-server.md), [AI Change Language](change-language.md), [Storage](storage.md).

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

### RemoteSignerPolicy

`RemoteSignerPolicy` define una allowlist local de signers remotos autorizados. La decisión se basa en la clave pública exacta (`public_key`) del `AgentIdentity`; `label` y `trust_tier` son metadata local para diagnóstico/auditoría, no material criptográfico.

```rust
pub struct RemoteSignerPolicy {
    pub allowed_signers: Vec<TrustedRemoteSigner>,
}

pub struct TrustedRemoteSigner {
    pub public_key: [u8; 32],
    pub trust_tier: SignerTrustTier,
    pub label: Option<String>,
}
```

El default seguro es `deny_all()`: un coordinator creado sin policy explícita rechaza submissions remotos válidamente firmados si el signer no está en la allowlist.

### RemoteConfig

`RemoteConfig` es el DTO serializable para cargar configuración remota de proyecto sin acoplarla a CLI ni transporte. Se puede leer desde JSON, CBOR u otro formato basado en serde, validarla, y convertirla a `RemoteSignerPolicy`.

La CLI reserva `.ail/remote.json` como archivo de configuración JSON del proyecto. Si el archivo no existe, el loader devuelve `RemoteConfig::default()`, que al convertirse en policy produce `RemoteSignerPolicy::deny_all()`.

```rust
pub struct RemoteConfig {
    pub allowed_signers: Vec<RemoteSignerConfig>,
    pub remotes: Vec<RemoteEndpointConfig>,
}

pub struct RemoteSignerConfig {
    pub public_key: String, // clave pública Ed25519, hex de 32 bytes
    pub trust_tier: SignerTrustTier,
    pub label: Option<String>,
}

pub struct RemoteEndpointConfig {
    pub name: String,
    pub endpoint: String,
}
```

Reglas:

```txt
1. `allowed_signers` vacío es válido y produce `RemoteSignerPolicy::deny_all()`.
2. Cada `public_key` debe ser hex de 64 caracteres.
3. Claves duplicadas se rechazan, incluso si difieren solo en mayúsculas/minúsculas.
4. `remotes` son hints de conexión; no otorgan autoridad criptográfica.
5. La conversión a policy usa solo `allowed_signers`.
```

### AgentKeypair

```rust
pub struct AgentKeypair { /* secret key field is private */ }

impl AgentKeypair {
    pub fn generate() -> Self
    pub fn identity(&self) -> AgentIdentity
    pub fn sign_bytes(&self, payload: &[u8]) -> [u8; 64]
}
```

La clave privada no se expone por el flujo normal de firma. `sign_bytes` devuelve 64 bytes raw Ed25519 (no DER).

### PlaintextDevSignerKeyMaterial

`PlaintextDevSignerKeyMaterial` es el formato serializable mínimo para persistir un signer local durante desarrollo. Guarda `secret_key_hex`, `public_key_hex`, `label` opcional, `version`, y un warning explícito: es texto plano para desarrollo local, no almacenamiento seguro de secretos de producción.

Al cargar, `to_keypair()` valida que la clave pública guardada coincida con la clave privada. Si no coincide, devuelve `SigningError::InvalidKeyMaterial`. Esto permite que una futura CLI use identidad durable contra `RemoteConfig.allowed_signers` sin cablear todavía lectura de archivos ni transporte.

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

`BundleStore` abstrae la retención de bundles aceptados. `InMemoryBundleStore` cubre coordinators efímeros y tests; `FileBundleStore` agrega una implementación durable en disco, con un archivo CBOR determinístico por `root` (`<root>.cbor`) bajo un directorio configurado. El store asume que el caller verificó integridad antes de escribir, y al leer vuelve a decodificar/verificar antes de devolver el bundle.

`ObjectBundle::from_store_with_snapshot_dependencies(root, store)` construye un bundle desde un objeto raíz y, si los bytes raíz decodifican como `SnapshotEnvelope`, agrega las dependencias directas almacenadas que la envelope declara: `graph_root_hash`, `parent_id`, `applied_change_id`, `audit_record_ids` y `migration_metadata_ids`. Los objetos de grafo siguen siendo opacos: no se promete traversal interno de `SemanticGraph` ni traversal transitivo general.

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

El `Coordinator` llama `verify_remote_submission(rcs)` que encadena verificación de firma → policy de signer → submit.

## RemoteError

```txt
RemoteError::SignatureInvalid
    La firma Ed25519 del envelope no pasó la verificación.
    El snapshot vivo no avanza.

RemoteError::SignerRejected(rejection)
    La firma era válida, pero el public_key del signer no está permitido por la policy local.
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

`RemoteExchangeRequest` / `RemoteExchangeResponse` definen el límite de servicio independiente del transporte para enviar changesets firmados e intercambiar bundles de objetos. El `Coordinator` acepta bundles enviados después de verificar su integridad, los retiene en un `BundleStore` en memoria por default, y responde pulls con `Bundle(bundle)` cuando conoce el `root` o `BundleMissing` cuando no existe. La implementación durable (`FileBundleStore`) queda disponible detrás del mismo trait; el wiring CLI/config es un paso separado.

La CLI implementa el primer slice de producto como `ail remote submit <change-id> --signer <key-ref> [--json]`. Este comando carga un ChangeSet local ya persistido, valida `.ail/remote.json` si existe, lo firma con una key efímera in-process etiquetada por `--signer`, allowlistea esa identidad solo para la invocación, y llama `Coordinator::handle_remote_exchange(RemoteExchangeRequest::SubmitChangeSet(_))`. El loader de `.ail/remote.json` ya existe para producir el default seguro `deny_all()` y validar JSON de proyecto, pero `remote submit` todavía no aplica esa policy al envío porque falta una identidad de signer durable que pueda coincidir con `allowed_signers`. No implementa transporte de red ni configuración durable de keys.

`ail remote push --root <object-id> [--json]` y `ail remote pull <root> [--json]` implementan un slice mínimo y honesto de bundles para proyectos inicializados con store de archivos. Para roots crudos, `push` carga solo el objeto root indicado desde `.ail/store/objects/`, arma un `ObjectBundle` de un objeto y reporta `bundle_scope=single_root_object`. Si el root decodifica como `SnapshotEnvelope`, el bundle agrega las dependencias directas disponibles que la envelope declara (`graph_root_hash`, `parent_id`, `applied_change_id`, `audit_record_ids` y `migration_metadata_ids`) y reporta `bundle_scope=root_with_snapshot_envelope_dependencies` solo cuando al menos una dependencia se incluyó. `pull` lee el bundle local, lo revalida con el exchange in-process (`PushBundle` + `PullBundle`), escribe los objetos en el object store local y reporta el scope del bundle real. El transporte se reporta como `local_file_bundle_store+in_process`: no hay red, discovery, uso de `.ail/remote.json` en push/pull, traversal interno de objetos de grafo, ni traversal transitivo general.

Referencias de código: `crates/ail-remote/src/identity.rs`, `crates/ail-remote/src/signing.rs`, `crates/ail-remote/src/bundle.rs`, `crates/ail-remote/src/exchange.rs`, `crates/ail-remote/src/crypto.rs`, `crates/ail-remote/src/error.rs`, `crates/ail-coordinator/src/coordinator.rs`, `crates/ail-cli/src/remote_config.rs`.
