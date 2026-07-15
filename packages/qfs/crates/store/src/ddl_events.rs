//! Pure model for replayable qfs DDL/config events.
//!
//! This is intentionally not the `/sys/audit` event model. Audit events are metadata-only and qfs
//! retains only a bounded local tail. DDL events are replay material: they carry normalized,
//! secret-free payload JSON that can rebuild qfs configuration state while current-state tables stay
//! efficient to query.

use qfs_crypto_core::sha256_hex;

const DOMAIN_TAG: &[u8] = b"qfs.ddl.event.v1";

/// Genesis predecessor hash for a fresh DDL event chain.
pub const GENESIS_PREV_HASH: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";

/// One replayable qfs DDL/config event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdlEvent {
    /// A caller-generated transaction/group id linking events emitted by one committed statement.
    pub tx_id: String,
    /// The acting principal label; never a credential.
    pub actor: String,
    /// RFC3339 UTC timestamp.
    pub ts: String,
    /// The normalized qfs state path affected, such as `/sys/drivers`.
    pub target_path: String,
    /// The write verb (`INSERT`/`UPSERT`/`UPDATE`/`REMOVE`/`CONNECT`/...).
    pub verb: String,
    /// Original DDL/config statement text when available. This is operator-authored text, not a
    /// credential channel; secrets must remain references.
    pub source_text: Option<String>,
    /// Normalized replay payload JSON. It may name secret references, never secret values.
    pub payload_json: String,
}

impl DdlEvent {
    /// Stable canonical bytes for content hashing.
    #[must_use]
    pub fn content(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        write_field(&mut buf, DOMAIN_TAG);
        write_field(&mut buf, self.tx_id.as_bytes());
        write_field(&mut buf, self.actor.as_bytes());
        write_field(&mut buf, self.ts.as_bytes());
        write_field(&mut buf, self.target_path.as_bytes());
        write_field(&mut buf, self.verb.as_bytes());
        match &self.source_text {
            Some(s) => {
                write_field(&mut buf, b"some");
                write_field(&mut buf, s.as_bytes());
            }
            None => write_field(&mut buf, b"none"),
        }
        write_field(&mut buf, self.payload_json.as_bytes());
        buf
    }

    /// `sha256_hex(content)` — stable over identical replay content.
    #[must_use]
    pub fn content_hash(&self) -> String {
        sha256_hex(&self.content())
    }

    /// Chain this event onto `prev_hash` at sequence `seq`.
    #[must_use]
    pub fn chain(self, seq: u64, prev_hash: impl Into<String>) -> ChainedDdlEvent {
        let prev_hash = prev_hash.into();
        let content_hash = self.content_hash();
        let hash = link_hash(&content_hash, &prev_hash);
        ChainedDdlEvent {
            seq,
            event: self,
            content_hash,
            prev_hash,
            hash,
        }
    }
}

/// One recorded DDL event plus its chain position and hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainedDdlEvent {
    pub seq: u64,
    pub event: DdlEvent,
    pub content_hash: String,
    pub prev_hash: String,
    pub hash: String,
}

/// Chain link hash shared by DDL event appenders.
#[must_use]
pub fn link_hash(content_hash: &str, prev_hash: &str) -> String {
    let mut buf = Vec::with_capacity(content_hash.len() + prev_hash.len());
    buf.extend_from_slice(content_hash.as_bytes());
    buf.extend_from_slice(prev_hash.as_bytes());
    sha256_hex(&buf)
}

/// Verify a contiguous DDL event chain or return the first divergent index.
#[must_use]
pub fn verify_chain(events: &[ChainedDdlEvent], genesis: &str) -> Option<usize> {
    let mut prev = genesis.to_string();
    for (i, ev) in events.iter().enumerate() {
        if ev.prev_hash != prev {
            return Some(i);
        }
        if ev.content_hash != ev.event.content_hash() {
            return Some(i);
        }
        if ev.hash != link_hash(&ev.content_hash, &ev.prev_hash) {
            return Some(i);
        }
        prev = ev.hash.clone();
    }
    None
}

fn write_field(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    buf.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(payload: &str) -> DdlEvent {
        DdlEvent {
            tx_id: "tx-1".into(),
            actor: "cli".into(),
            ts: "2026-07-07T00:00:00Z".into(),
            target_path: "/sys/drivers".into(),
            verb: "INSERT".into(),
            source_text: Some("CREATE DRIVER chatwork AT 'https://api.chatwork.com/v2' AUTH HEADER 'x-chatworktoken'".into()),
            payload_json: payload.into(),
        }
    }

    #[test]
    fn content_hash_is_stable_and_field_sensitive() {
        let a = event(r#"{"kind":"driver","name":"chatwork"}"#);
        assert_eq!(a.content_hash(), a.content_hash());
        assert_eq!(a.content_hash().len(), 64);
        let b = event(r#"{"kind":"driver","name":"slack"}"#);
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn verify_chain_detects_tampering_and_deletion() {
        let a = event(r#"{"name":"a"}"#).chain(1, GENESIS_PREV_HASH);
        let b = event(r#"{"name":"b"}"#).chain(2, a.hash.clone());
        let chain = vec![a.clone(), b.clone()];
        assert_eq!(verify_chain(&chain, GENESIS_PREV_HASH), None);

        let mut tampered = chain.clone();
        tampered[1].event.payload_json = r#"{"name":"HACKED"}"#.into();
        assert_eq!(verify_chain(&tampered, GENESIS_PREV_HASH), Some(1));

        let pruned = vec![b];
        assert_eq!(verify_chain(&pruned, GENESIS_PREV_HASH), Some(0));
    }

    #[test]
    fn normal_driver_payload_example_is_secret_free() {
        let e = event(r#"{"kind":"driver","auth":{"kind":"header","name":"x-chatworktoken"}}"#);
        let dump = format!("{e:?}");
        assert!(!dump.contains("SUPER-SECRET-TOKEN"));
        assert!(!dump.contains("ghp_"));
    }
}
