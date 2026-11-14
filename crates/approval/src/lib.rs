//! What an approval signs, and how an agent or the controller checks the
//! passkey assertion that signs it.
//!
//! An approval names a change and, for each host it covers, the agent
//! instance, the hash of the config, and the hash of the plan the person
//! read. Its WebAuthn challenge is the SHA-256 of [`Approval::signed_text`].
//! The agent checks the assertion against the approver keys pinned at
//! install, so the controller cannot make it apply anything without a
//! signature from one of those keys.
//!
//! Only ES256 (ECDSA on P-256 with SHA-256) keys are supported.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::pkcs8::DecodePublicKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// The custom capability that carries approvals to the agent and apply
/// results back.
pub const CAPABILITY: &str = "io.sewa.approval";
/// The custom message type of an approval, from the controller.
pub const APPROVAL_TYPE: &str = "approval";
/// The custom message type of an apply result, from the agent.
pub const RESULT_TYPE: &str = "result";

/// The COSE algorithm number of ES256.
pub const ES256: i64 = -7;

/// The first line of every signed text, so a signature never means
/// anything else.
const HEADER: &str = "sewa approval v1";

/// One host an approval covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostPlan {
    pub host_id: Uuid,
    /// The agent instance that planned it. An agent finds its own entry by
    /// its instance UID, which only it chooses.
    pub instance_uid: Uuid,
    /// The hex SHA-256 of the config the host applies.
    pub config_hash: String,
    /// The hex SHA-256 of the plan the person approved.
    pub plan_hash: String,
}

/// What a person approves: one change, and the plan of each host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub change_id: Uuid,
    pub hosts: Vec<HostPlan>,
}

impl Approval {
    /// The text the challenge hashes: a header, the change, then one line
    /// per host in host id order.
    pub fn signed_text(&self) -> String {
        let mut hosts: Vec<&HostPlan> = self.hosts.iter().collect();
        hosts.sort_by_key(|host| host.host_id);
        let mut text = format!("{HEADER}\nchange {}\n", self.change_id);
        for host in hosts {
            text.push_str(&format!(
                "host {} instance {} config {} plan {}\n",
                host.host_id, host.instance_uid, host.config_hash, host.plan_hash
            ));
        }
        text
    }

    /// The WebAuthn challenge of this approval.
    pub fn challenge(&self) -> [u8; 32] {
        Sha256::digest(self.signed_text().as_bytes()).into()
    }

    /// The entry of an agent instance.
    pub fn host(&self, instance_uid: Uuid) -> Option<&HostPlan> {
        self.hosts
            .iter()
            .find(|host| host.instance_uid == instance_uid)
    }
}

/// A WebAuthn assertion, each field in unpadded base64url as the browser's
/// bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assertion {
    pub credential_id: String,
    pub authenticator_data: String,
    pub client_data_json: String,
    pub signature: String,
}

/// An approval with its id and the assertion that signs it, as the
/// controller sends it to an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signed {
    pub approval_id: Uuid,
    #[serde(flatten)]
    pub approval: Approval,
    pub assertion: Assertion,
}

/// What an agent did with an approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub approval_id: Uuid,
    pub change_id: Uuid,
    /// `applied`, `failed`, or `replan`.
    pub status: String,
    /// The hash of the plan the agent found before applying.
    pub plan_hash: Option<String>,
    /// mise's output, or why the agent refused.
    pub log: String,
    /// When the agent finished with the approval, in Unix milliseconds by
    /// its clock. A result the agent buffered while the controller was
    /// down arrives later than this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<i64>,
}

/// One approver key, as the agent pins it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approver {
    /// The credential id, unpadded base64url.
    pub id: String,
    /// The public key as SubjectPublicKeyInfo DER, unpadded base64url.
    pub public_key: String,
    /// Who registered it.
    pub login: String,
}

/// The approver keys an agent trusts, and where the passkeys belong.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pinned {
    /// The WebAuthn relying party id: the controller's host name.
    pub rp_id: String,
    /// The web app's origin, such as `https://sewa.example.com`.
    pub origin: String,
    pub keys: Vec<Approver>,
}

/// Why an assertion was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// No approver key is pinned.
    NoKeys,
    /// The credential is not one of the pinned keys.
    UnknownKey,
    /// A field does not decode.
    Malformed,
    WrongType,
    WrongChallenge,
    WrongOrigin,
    WrongRpId,
    /// The authenticator did not see a person.
    NotPresent,
    /// The authenticator did not verify the person.
    NotVerified,
    BadSignature,
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoKeys => "no approver key is pinned",
            Self::UnknownKey => "the key is not a pinned approver key",
            Self::Malformed => "the assertion does not decode",
            Self::WrongType => "the client data is not of the expected type",
            Self::WrongChallenge => "the challenge does not match",
            Self::WrongOrigin => "the origin does not match",
            Self::WrongRpId => "the relying party does not match",
            Self::NotPresent => "the authenticator saw no user",
            Self::NotVerified => "the authenticator did not verify the user",
            Self::BadSignature => "the signature does not verify",
        })
    }
}

pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode(text: &str) -> Result<Vec<u8>, Rejection> {
    URL_SAFE_NO_PAD
        .decode(text.trim_end_matches('='))
        .map_err(|_| Rejection::Malformed)
}

/// Reads an ES256 public key from unpadded base64url SPKI DER.
pub fn public_key(text: &str) -> Result<VerifyingKey, Rejection> {
    VerifyingKey::from_public_key_der(&decode(text)?).map_err(|_| Rejection::Malformed)
}

/// Checks the client data of a ceremony: its type (`webauthn.create` or
/// `webauthn.get`), its challenge, and its origin.
pub fn check_client_data(
    client_data_json: &[u8],
    kind: &str,
    challenge: &[u8],
    origin: &str,
) -> Result<(), Rejection> {
    #[derive(Deserialize)]
    struct ClientData {
        #[serde(rename = "type")]
        kind: String,
        challenge: String,
        origin: String,
        #[serde(rename = "crossOrigin", default)]
        cross_origin: bool,
    }
    let data: ClientData =
        serde_json::from_slice(client_data_json).map_err(|_| Rejection::Malformed)?;
    if data.kind != kind {
        return Err(Rejection::WrongType);
    }
    if decode(&data.challenge)? != challenge {
        return Err(Rejection::WrongChallenge);
    }
    if data.origin != origin || data.cross_origin {
        return Err(Rejection::WrongOrigin);
    }
    Ok(())
}

/// Checks an assertion of `challenge` against the pinned keys: the key, the
/// client data, the relying party, that the user was present and verified,
/// and the signature over the authenticator data and the client data hash.
/// Returns the approver that signed.
pub fn verify<'a>(
    pinned: &'a Pinned,
    assertion: &Assertion,
    challenge: &[u8],
) -> Result<&'a Approver, Rejection> {
    if pinned.keys.is_empty() {
        return Err(Rejection::NoKeys);
    }
    let approver = pinned
        .keys
        .iter()
        .find(|key| key.id == assertion.credential_id.trim_end_matches('='))
        .ok_or(Rejection::UnknownKey)?;
    let key = public_key(&approver.public_key)?;
    let client_data = decode(&assertion.client_data_json)?;
    check_client_data(&client_data, "webauthn.get", challenge, &pinned.origin)?;
    let authenticator_data = decode(&assertion.authenticator_data)?;
    if authenticator_data.len() < 37 {
        return Err(Rejection::Malformed);
    }
    let rp_id_hash = Sha256::digest(pinned.rp_id.as_bytes());
    if authenticator_data[..32] != rp_id_hash[..] {
        return Err(Rejection::WrongRpId);
    }
    let flags = authenticator_data[32];
    if flags & 0x01 == 0 {
        return Err(Rejection::NotPresent);
    }
    if flags & 0x04 == 0 {
        return Err(Rejection::NotVerified);
    }
    let signature =
        Signature::from_der(&decode(&assertion.signature)?).map_err(|_| Rejection::BadSignature)?;
    let mut signed = authenticator_data;
    signed.extend_from_slice(&Sha256::digest(&client_data));
    key.verify(&signed, &signature)
        .map_err(|_| Rejection::BadSignature)?;
    Ok(approver)
}

/// A software authenticator, for tests on both sides.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::{Signature, SigningKey};
    use p256::pkcs8::EncodePublicKey;
    use sha2::{Digest, Sha256};

    use super::{Approver, Assertion, encode};

    pub struct Authenticator {
        pub id: Vec<u8>,
        key: SigningKey,
    }

    impl Authenticator {
        /// An authenticator whose private key is `seed` repeated.
        pub fn new(seed: u8) -> Self {
            Self {
                id: vec![seed; 16],
                key: SigningKey::from_slice(&[seed; 32]).expect("a valid scalar"),
            }
        }

        pub fn public_key(&self) -> String {
            let der = self
                .key
                .verifying_key()
                .to_public_key_der()
                .expect("a P-256 key encodes");
            encode(der.as_bytes())
        }

        pub fn approver(&self, login: &str) -> Approver {
            Approver {
                id: encode(&self.id),
                public_key: self.public_key(),
                login: login.to_owned(),
            }
        }

        /// The client data JSON of a ceremony.
        pub fn client_data(kind: &str, challenge: &[u8], origin: &str) -> Vec<u8> {
            serde_json::json!({
                "type": kind,
                "challenge": encode(challenge),
                "origin": origin,
                "crossOrigin": false,
            })
            .to_string()
            .into_bytes()
        }

        /// Signs `challenge` for `rp_id` and `origin`, with the user present
        /// and verified.
        pub fn assert(&self, rp_id: &str, origin: &str, challenge: &[u8]) -> Assertion {
            let client_data = Self::client_data("webauthn.get", challenge, origin);
            let mut authenticator_data = Sha256::digest(rp_id.as_bytes()).to_vec();
            authenticator_data.push(0x05);
            authenticator_data.extend_from_slice(&[0, 0, 0, 1]);
            let mut signed = authenticator_data.clone();
            signed.extend_from_slice(&Sha256::digest(&client_data));
            let signature: Signature = self.key.sign(&signed);
            Assertion {
                credential_id: encode(&self.id),
                authenticator_data: encode(&authenticator_data),
                client_data_json: encode(&client_data),
                signature: encode(signature.to_der().as_bytes()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::Authenticator;
    use super::*;

    const RP_ID: &str = "sewa.example.com";
    const ORIGIN: &str = "https://sewa.example.com";

    fn approval() -> Approval {
        Approval {
            change_id: Uuid::from_u128(1),
            hosts: vec![
                HostPlan {
                    host_id: Uuid::from_u128(3),
                    instance_uid: Uuid::from_u128(30),
                    config_hash: "c3".to_owned(),
                    plan_hash: "p3".to_owned(),
                },
                HostPlan {
                    host_id: Uuid::from_u128(2),
                    instance_uid: Uuid::from_u128(20),
                    config_hash: "c2".to_owned(),
                    plan_hash: "p2".to_owned(),
                },
            ],
        }
    }

    fn pinned(authenticator: &Authenticator) -> Pinned {
        Pinned {
            rp_id: RP_ID.to_owned(),
            origin: ORIGIN.to_owned(),
            keys: vec![authenticator.approver("admin@example.com")],
        }
    }

    #[test]
    fn the_signed_text_lists_hosts_in_order() {
        assert_eq!(
            approval().signed_text(),
            "sewa approval v1\n\
             change 00000000-0000-0000-0000-000000000001\n\
             host 00000000-0000-0000-0000-000000000002 instance 00000000-0000-0000-0000-000000000014 config c2 plan p2\n\
             host 00000000-0000-0000-0000-000000000003 instance 00000000-0000-0000-0000-00000000001e config c3 plan p3\n"
        );
    }

    #[test]
    fn a_signed_approval_verifies() {
        let authenticator = Authenticator::new(7);
        let challenge = approval().challenge();
        let assertion = authenticator.assert(RP_ID, ORIGIN, &challenge);
        let pinned = pinned(&authenticator);
        let approver = verify(&pinned, &assertion, &challenge).unwrap();
        assert_eq!(approver.login, "admin@example.com");
    }

    #[test]
    fn refuses_what_does_not_match() {
        let authenticator = Authenticator::new(7);
        let pinned = pinned(&authenticator);
        let challenge = approval().challenge();
        let good = authenticator.assert(RP_ID, ORIGIN, &challenge);

        assert_eq!(
            verify(&Pinned::default(), &good, &challenge),
            Err(Rejection::NoKeys)
        );
        let other = Authenticator::new(9).assert(RP_ID, ORIGIN, &challenge);
        assert_eq!(
            verify(&pinned, &other, &challenge),
            Err(Rejection::UnknownKey)
        );
        // Another key under a pinned credential id: a forgery.
        let forged = Assertion {
            credential_id: good.credential_id.clone(),
            ..other
        };
        assert_eq!(
            verify(&pinned, &forged, &challenge),
            Err(Rejection::BadSignature)
        );
        assert_eq!(
            verify(&pinned, &good, &[0; 32]),
            Err(Rejection::WrongChallenge)
        );
        let wrong_rp = authenticator.assert("other.example.com", ORIGIN, &challenge);
        assert_eq!(
            verify(&pinned, &wrong_rp, &challenge),
            Err(Rejection::WrongRpId)
        );
        let wrong_origin = authenticator.assert(RP_ID, "https://other.example.com", &challenge);
        assert_eq!(
            verify(&pinned, &wrong_origin, &challenge),
            Err(Rejection::WrongOrigin)
        );
        // Client data swapped after signing.
        let swapped = Assertion {
            client_data_json: encode(&Authenticator::client_data(
                "webauthn.get",
                &challenge,
                ORIGIN,
            )),
            ..authenticator.assert(RP_ID, ORIGIN, &[1; 32])
        };
        assert_eq!(
            verify(&pinned, &swapped, &challenge),
            Err(Rejection::BadSignature)
        );
        let mut flags = decode(&good.authenticator_data).unwrap();
        flags[32] = 0x01;
        let unverified = Assertion {
            authenticator_data: encode(&flags),
            ..good.clone()
        };
        assert_eq!(
            verify(&pinned, &unverified, &challenge),
            Err(Rejection::NotVerified)
        );
    }
}
