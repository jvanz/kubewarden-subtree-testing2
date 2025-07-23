use oci_client::{manifest::WASM_LAYER_MEDIA_TYPE, secrets::RegistryAuth, Reference};
use sigstore::{
    cosign::{self, signature_layers::SignatureLayer, ClientBuilder, CosignCapabilities},
    errors::SigstoreError,
    registry::oci_reference::OciReference,
    trust::ManualTrustRoot,
};
use std::{convert::TryFrom, str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::{
    errors::FailedToParseYamlDataError,
    policy::Policy,
    registry::build_fully_resolved_reference,
    sources::Sources,
    verify::{
        config::Signature,
        errors::{VerifyError, VerifyResult},
    },
    Registry,
};

pub mod config;
pub mod errors;
pub mod verification_constraints;

/// This structure simplifies the process of policy verification
/// using Sigstore
#[derive(Clone)]
pub struct Verifier {
    cosign_client: Arc<Mutex<sigstore::cosign::Client>>,
    sources: Option<Sources>,
}

impl Verifier {
    /// Creates a new verifier that leverages an already existing
    /// Cosign client.
    pub fn new_from_cosign_client(
        cosign_client: Arc<Mutex<sigstore::cosign::Client>>,
        sources: Option<Sources>,
    ) -> Self {
        Self {
            cosign_client,
            sources,
        }
    }

    /// Creates a new verifier using the `Sources` provided. These are
    /// later used to interact with remote OCI registries.
    pub async fn new(
        sources: Option<Sources>,
        trust_root: Option<Arc<ManualTrustRoot<'static>>>,
    ) -> VerifyResult<Self> {
        let client_config: sigstore::registry::ClientConfig =
            sources.clone().unwrap_or_default().into();
        let mut cosign_client_builder = ClientBuilder::default()
            .with_oci_client_config(client_config)
            .enable_registry_caching();
        let cosign_client = match trust_root {
            Some(trust_root) => {
                cosign_client_builder =
                    cosign_client_builder.with_trust_repository(trust_root.as_ref())?;
                cosign_client_builder.build()?
            }
            None => {
                warn!("Sigstore Verifier created without Fulcio data: keyless signatures are going to be discarded because they cannot be verified");
                warn!("Sigstore Verifier created without Rekor data: transparency log data won't be used");
                warn!("Sigstore capabilities are going to be limited");

                cosign_client_builder.build()?
            }
        };

        Ok(Verifier {
            cosign_client: Arc::new(Mutex::new(cosign_client)),
            sources,
        })
    }

    /// Verifies the given policy using the LatestVerificationConfig provided by
    /// the user.
    ///
    /// In case of success, returns the manifest digest of the verified policy.
    ///
    /// Note well: this method doesn't compare the checksum of a possible local
    /// file with the one inside of the signed (and verified) manifest, as that
    /// can only be done with certainty after pulling the policy.
    ///
    /// Note well: right now, verification can be done only against policies
    /// that are stored inside of OCI registries.
    pub async fn verify(
        &mut self,
        image_url: &str,
        verification_config: &config::LatestVerificationConfig,
    ) -> VerifyResult<String> {
        let (source_image_digest, trusted_layers) =
            fetch_sigstore_remote_data(&self.cosign_client, image_url).await?;

        // verify signatures against our config:
        //
        verify_signatures_against_config(verification_config, &trusted_layers)?;

        // everything is fine here:
        debug!(
            policy = image_url.to_string().as_str(),
            "Policy successfully verified"
        );
        Ok(source_image_digest)
    }

    /// Verifies the checksum of the local file by comparing it with the one
    /// mentioned inside of the signed (and verified) manifest digest.
    /// This ensures nobody tampered with the local policy.
    ///
    /// Note well: right now, verification can be done only against policies
    /// that are stored inside of OCI registries.
    pub async fn verify_local_file_checksum(
        &mut self,
        policy: &Policy,
        verified_manifest_digest: &str,
    ) -> VerifyResult<()> {
        let image_name = match policy.uri.strip_prefix("registry://") {
            None => policy.uri.as_str(),
            Some(url) => url,
        };
        if let Err(e) = Reference::try_from(image_name) {
            return Err(VerifyError::InvalidOCIImageReferenceError(e));
        }

        if !policy.local_path.exists() {
            return Err(VerifyError::MissingWasmFileError(
                policy.local_path.display().to_string(),
            ));
        }

        let registry = crate::registry::Registry::new();
        let reference = oci_client::Reference::from_str(image_name)?;
        let image_immutable_ref = format!(
            "registry://{}/{}@{}",
            reference.registry(),
            reference.repository(),
            verified_manifest_digest
        );
        let manifest = registry
            .manifest(&image_immutable_ref, self.sources.as_ref())
            .await?;

        let digests: Vec<String> = if let oci_client::manifest::OciManifest::Image(ref image) =
            manifest
        {
            image
                .layers
                .iter()
                .filter_map(|layer| match layer.media_type.as_str() {
                    WASM_LAYER_MEDIA_TYPE => Some(layer.digest.clone()),
                    _ => None,
                })
                .collect()
        } else {
            unreachable!("Expected Image, found ImageIndex manifest. This cannot happen, as oci clientConfig.platform_resolver is None and we will error earlier");
        };

        if digests.len() != 1 {
            error!(manifest = ?manifest, "The manifest is expected to have one WASM layer");
            return Err(VerifyError::ChecksumVerificationError("Cannot verify local file integrity, the remote manifest doesn't have only one WASM layer".to_owned()));
        }
        let expected_digest = digests[0]
            .strip_prefix("sha256:")
            .ok_or_else(|| VerifyError::ChecksumVerificationError("The checksum inside of the remote manifest is not using the sha256 hashing algorithm as expected.".to_owned()))?;

        let file_digest = policy.digest()?;
        if file_digest != expected_digest {
            Err(VerifyError::ChecksumVerificationError(format!("The digest of the local file doesn't match with the one reported inside of the signed manifest. Got {file_digest} instead of {expected_digest}")))
        } else {
            info!("Local file checksum verification passed");
            Ok(())
        }
    }
}

/// Verifies the trusted layers against the VerificationConfig passed to it.
/// It does that by creating the verification constraints from the config, and
/// then filtering the trusted_layers with the corresponding constraints.
fn verify_signatures_against_config(
    verification_config: &config::LatestVerificationConfig,
    trusted_layers: &[SignatureLayer],
) -> VerifyResult<()> {
    // filter trusted_layers against our verification constraints:
    //
    if verification_config.all_of.is_none() && verification_config.any_of.is_none() {
        // deserialized config is already sanitized, and should not reach here anyways
        return Err(VerifyError::ImageVerificationError(
            "Image verification failed: no signatures to verify".to_owned(),
        ));
    }

    use rayon::prelude::*;

    if let Some(ref signatures_all_of) = verification_config.all_of {
        let unsatisfied_signatures: Vec<&Signature> = signatures_all_of
            .par_iter()
            .filter(|signature| match signature.verifier() {
                Ok(verifier) => {
                    let constraints = [verifier];
                    let is_satisfied =
                        cosign::verify_constraints(trusted_layers, constraints.iter());
                    match is_satisfied {
                        Ok(_) => {
                            debug!(
                                "Constraint satisfied:\n{}",
                                &serde_yaml::to_string(signature).unwrap()
                            );
                            false
                        }
                        Err(_) => true, //filter into unsatisfied_signatures
                    }
                }
                Err(error) => {
                    info!(?error, ?signature, "Cannot create verifier for signature");
                    true
                }
            })
            .collect();
        if !unsatisfied_signatures.is_empty() {
            let mut errormsg = "Image verification failed: missing signatures\n".to_string();
            errormsg.push_str("The following constraints were not satisfied:\n");
            for s in unsatisfied_signatures {
                errormsg.push_str(&serde_yaml::to_string(s).map_err(FailedToParseYamlDataError)?);
            }
            return Err(VerifyError::ImageVerificationError(errormsg));
        }
    }

    if let Some(ref signatures_any_of) = verification_config.any_of {
        let unsatisfied_signatures: Vec<&Signature> = signatures_any_of
            .signatures
            .par_iter()
            .filter(|signature| match signature.verifier() {
                Ok(verifier) => {
                    let constraints = [verifier];
                    cosign::verify_constraints(trusted_layers, constraints.iter()).is_err()
                }
                Err(error) => {
                    info!(?error, ?signature, "Cannot create verifier for signature");
                    true
                }
            })
            .collect();
        {
            let num_satisfied_constraints =
                signatures_any_of.signatures.len() - unsatisfied_signatures.len();
            let minimum_matches: usize = signatures_any_of.minimum_matches.into();

            if num_satisfied_constraints < minimum_matches {
                let mut errormsg =
                    format!("Image verification failed: minimum number of signatures not reached: needed {}, got {}", signatures_any_of.minimum_matches, num_satisfied_constraints);
                errormsg.push_str("\nThe following constraints were not satisfied:\n");
                for s in unsatisfied_signatures.iter() {
                    errormsg
                        .push_str(&serde_yaml::to_string(s).map_err(FailedToParseYamlDataError)?);
                }
                return Err(VerifyError::ImageVerificationError(errormsg));
            }
        }
    }
    Ok(())
}

/// Fetch the sigstore signature data
/// Returns:
/// * String holding the source image digest
/// * List of signature layers
pub async fn fetch_sigstore_remote_data(
    cosign_client_input: &Arc<Mutex<cosign::Client>>,
    image_url: &str,
) -> VerifyResult<(String, Vec<SignatureLayer>)> {
    let mut cosign_client = cosign_client_input.lock().await;

    // obtain registry auth:
    let reference = build_fully_resolved_reference(image_url)?;
    let auth = Registry::auth(reference.registry());

    let sigstore_auth = match auth {
        RegistryAuth::Anonymous => sigstore::registry::Auth::Anonymous,
        RegistryAuth::Basic(username, password) => {
            sigstore::registry::Auth::Basic(username, password)
        }
        RegistryAuth::Bearer(token) => sigstore::registry::Auth::Bearer(token),
    };

    // obtain all signatures of image:
    //
    // trusted_signature_layers() will error early if cosign_client using
    // Fulcio,Rekor certs and signatures are not verified
    let image_name = reference.whole();
    let image_oci_ref = OciReference::from_str(&image_name)
        .map_err(VerifyError::FailedToFetchTrustedLayersError)?;
    let (cosign_signature_image, source_image_digest) = cosign_client
        .triangulate(&image_oci_ref, &sigstore_auth)
        .await
        .map_err(VerifyError::FailedToFetchTrustedLayersError)?;

    // get trusted layers
    let layers = cosign_client
        .trusted_signature_layers(
            &sigstore_auth,
            &source_image_digest,
            &cosign_signature_image,
        )
        .await
        .map_err(|e| match e {
            SigstoreError::RegistryPullManifestError { image: _, error: _ } => {
                VerifyError::ImageVerificationError(format!(
                    "no signatures found for image: {image_name} "
                ))
            }
            e => VerifyError::FailedToFetchTrustedLayersError(e),
        })?;
    Ok((source_image_digest, layers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::{AnyOf, LatestVerificationConfig, Signature, Subject};
    use cosign::signature_layers::CertificateSubject;
    use sigstore::{
        cosign::payload::simple_signing::SimpleSigning,
        cosign::signature_layers::CertificateSignature,
    };

    fn build_signature_layers_keyless(
        issuer: Option<String>,
        subject: CertificateSubject,
    ) -> SignatureLayer {
        let pub_key = r#"-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAELKhD7F5OKy77Z582Y6h0u1J3GNA+
kvUsh4eKpd1lwkDAzfFDs7yXEExsEkPPuiQJBelDT68n7PDIWB/QEY7mrA==
-----END PUBLIC KEY-----"#;
        let verification_key =
            sigstore::crypto::CosignVerificationKey::try_from_pem(pub_key.as_bytes())
                .expect("Cannot create CosignVerificationKey");

        let raw_data = r#"{"critical":{"identity":{"docker-reference":"registry-testing.svc.lan/kubewarden/disallow-service-nodeport"},"image":{"docker-manifest-digest":"sha256:5f481572d088dc4023afb35fced9530ced3d9b03bf7299c6f492163cb9f0452e"},"type":"cosign container image signature"},"optional":null}"#;
        let raw_data = raw_data.as_bytes().to_vec();
        let signature = "MEUCIGqWScz7s9aP2sGXNFKeqivw3B6kPRs56AITIHnvd5igAiEA1kzbaV2Y5yPE81EN92NUFOl31LLJSvwsjFQ07m2XqaA=".to_string();

        let simple_signing: SimpleSigning =
            serde_json::from_slice(&raw_data).expect("Cannot deserialize SimpleSigning");

        let certificate_signature = Some(CertificateSignature {
            verification_key,
            issuer,
            subject,
            github_workflow_trigger: None,
            github_workflow_sha: None,
            github_workflow_name: None,
            github_workflow_repository: None,
            github_workflow_ref: None,
        });

        SignatureLayer {
            simple_signing,
            oci_digest: "not relevant".to_string(),
            certificate_signature,
            bundle: None,
            signature: Some(signature),
            raw_data,
        }
    }

    fn generic_issuer(issuer: &str, subject_str: &str) -> config::Signature {
        let subject = Subject::Equal(subject_str.to_string());
        Signature::GenericIssuer {
            issuer: issuer.to_string(),
            subject,
            annotations: None,
        }
    }

    fn signature_layer(issuer: &str, subject_str: &str) -> SignatureLayer {
        let certificate_subject = CertificateSubject::Email(subject_str.to_string());
        build_signature_layers_keyless(Some(issuer.to_string()), certificate_subject)
    }

    #[test]
    fn test_verify_config() {
        // build verification config:
        let signatures_all_of: Vec<Signature> = vec![generic_issuer(
            "https://github.com/login/oauth",
            "user1@provider.com",
        )];
        let signatures_any_of: Vec<Signature> = vec![generic_issuer(
            "https://github.com/login/oauth",
            "user2@provider.com",
        )];
        let verification_config = LatestVerificationConfig {
            all_of: Some(signatures_all_of),
            any_of: Some(AnyOf {
                minimum_matches: 1,
                signatures: signatures_any_of,
            }),
        };

        // build trusted layers:
        let trusted_layers: Vec<SignatureLayer> = vec![
            signature_layer("https://github.com/login/oauth", "user1@provider.com"),
            signature_layer("https://github.com/login/oauth", "user2@provider.com"),
        ];

        assert!(verify_signatures_against_config(&verification_config, &trusted_layers).is_ok());
    }

    //#[should_panic(expected = "Image verification failed: no signatures to verify")]
    #[test]
    fn test_verify_config_missing_both_any_of_all_of() {
        // build verification config:
        let verification_config = LatestVerificationConfig {
            all_of: None,
            any_of: None,
        };

        // build trusted layers:
        let trusted_layers: Vec<SignatureLayer> = vec![signature_layer(
            "https://github.com/login/oauth",
            "user-unrelated@provider.com",
        )];

        let error = verify_signatures_against_config(&verification_config, &trusted_layers);
        let expected_msg = "Image verification failed: no signatures to verify";
        assert!(
            matches!(error, Err(VerifyError::ImageVerificationError(msg)) if msg == expected_msg)
        );
    }

    #[test]
    fn test_verify_config_not_matching_all_of() {
        // build verification config:
        let signatures_all_of: Vec<Signature> = vec![generic_issuer(
            "https://github.com/login/oauth",
            "user1@provider.com",
        )];
        let verification_config = LatestVerificationConfig {
            all_of: Some(signatures_all_of),
            any_of: None,
        };

        // build trusted layers:
        let trusted_layers: Vec<SignatureLayer> = vec![signature_layer(
            "https://github.com/login/oauth",
            "user-unrelated@provider.com",
        )];

        let error = verify_signatures_against_config(&verification_config, &trusted_layers);
        assert!(error.is_err());
        let expected_msg = r#"Image verification failed: missing signatures
The following constraints were not satisfied:
kind: genericIssuer
issuer: https://github.com/login/oauth
subject: !equal user1@provider.com
annotations: null
"#;
        assert!(
            matches!(error, Err(VerifyError::ImageVerificationError(msg)) if msg == expected_msg)
        );
    }

    #[test]
    fn test_verify_config_missing_signatures_all_of() {
        // build verification config:
        let signatures_all_of: Vec<Signature> = vec![
            generic_issuer("https://github.com/login/oauth", "user1@provider.com"),
            generic_issuer("https://github.com/login/oauth", "user2@provider.com"),
            generic_issuer("https://github.com/login/oauth", "user3@provider.com"),
        ];
        let verification_config = LatestVerificationConfig {
            all_of: Some(signatures_all_of),
            any_of: None,
        };

        // build trusted layers:
        let trusted_layers: Vec<SignatureLayer> = vec![
            signature_layer("https://github.com/login/oauth", "user1@provider.com"),
            signature_layer("https://github.com/login/oauth", "user2@provider.com"),
        ];

        let error = verify_signatures_against_config(&verification_config, &trusted_layers);
        assert!(error.is_err());
        let expected_msg = r#"Image verification failed: missing signatures
The following constraints were not satisfied:
kind: genericIssuer
issuer: https://github.com/login/oauth
subject: !equal user3@provider.com
annotations: null
"#;
        assert!(
            matches!(error, Err(VerifyError::ImageVerificationError(msg)) if msg == expected_msg)
        );
    }

    #[test]
    fn test_verify_config_missing_signatures_any_of() {
        // build verification config:
        let signatures_any_of: Vec<Signature> = vec![
            generic_issuer("https://github.com/login/oauth", "user1@provider.com"),
            generic_issuer("https://github.com/login/oauth", "user2@provider.com"),
            generic_issuer("https://github.com/login/oauth", "user3@provider.com"),
        ];
        let verification_config = LatestVerificationConfig {
            all_of: None,
            any_of: Some(AnyOf {
                minimum_matches: 2,
                signatures: signatures_any_of,
            }),
        };

        // build trusted layers:
        let trusted_layers: Vec<SignatureLayer> = vec![signature_layer(
            "https://github.com/login/oauth",
            "user1@provider.com",
        )];

        let error = verify_signatures_against_config(&verification_config, &trusted_layers);
        let expected_msg = r#"Image verification failed: minimum number of signatures not reached: needed 2, got 1
The following constraints were not satisfied:
kind: genericIssuer
issuer: https://github.com/login/oauth
subject: !equal user2@provider.com
annotations: null
kind: genericIssuer
issuer: https://github.com/login/oauth
subject: !equal user3@provider.com
annotations: null
"#;
        assert!(
            matches!(error, Err(VerifyError::ImageVerificationError(msg)) if msg == expected_msg)
        );
    }

    #[test]
    fn test_verify_config_quorum_signatures_any_of() {
        // build verification config:
        let signatures_any_of: Vec<Signature> = vec![
            generic_issuer("https://github.com/login/oauth", "user1@provider.com"),
            generic_issuer("https://github.com/login/oauth", "user2@provider.com"),
            generic_issuer("https://github.com/login/oauth", "user3@provider.com"),
        ];
        let verification_config = LatestVerificationConfig {
            all_of: None,
            any_of: Some(AnyOf {
                minimum_matches: 2,
                signatures: signatures_any_of,
            }),
        };

        // build trusted layers:
        let trusted_layers: Vec<SignatureLayer> = vec![
            signature_layer("https://github.com/login/oauth", "user1@provider.com"),
            signature_layer("https://github.com/login/oauth", "user2@provider.com"),
        ];

        assert!(verify_signatures_against_config(&verification_config, &trusted_layers).is_ok());
    }
}
