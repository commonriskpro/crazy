use crate::manifest::PackageManifest;
use crate::signing::SignedPackage;

/// Extension that registers a `SignedPackage` into a `PackageRegistry` after
/// verifying its signature.
///
/// This wires signing into the publish workflow: callers cannot bypass
/// signature verification when publishing through this API.
pub fn publish_signed(
    registry: &mut crate::registry::PackageRegistry,
    signed: &SignedPackage,
) -> Result<PackageManifest, crate::signing::SigningError> {
    let manifest = signed.manifest.clone();
    registry.register_signed(signed.clone())?;
    Ok(manifest)
}
