//! Alibaba Cloud OSS V4 presigning for browser uploads and sandbox downloads.
//!
//! Only `ring` is used for cryptographic primitives. Secrets never leave
//! `SecretString`, and Debug output redacts both the AccessKey ID and secret.

use std::{
    env,
    fmt::{Debug, Formatter, Write as _},
};

use chrono::{DateTime, Utc};
use ring::{digest, hmac};
use secrecy::{ExposeSecret, SecretString};

use crate::error::{RuntimeError, RuntimeResult};

pub const DEFAULT_KEY_PREFIX: &str = "quire/attachments";
const ALGORITHM: &str = "OSS4-HMAC-SHA256";
const SERVICE: &str = "oss";
const REQUEST_SUFFIX: &str = "aliyun_v4_request";
const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";
const MAX_EXPIRES_SECS: u64 = 604_800;

/// OSS settings. A partially populated configuration stays in local attachment
/// mode because [`OssConfig::is_configured`] requires every signing input.
#[derive(Clone)]
pub struct OssConfig {
    pub bucket: Option<String>,
    pub region: Option<String>,
    /// Browser-facing endpoint used for presigned PUT uploads.
    pub public_endpoint: Option<String>,
    /// Optional VPC endpoint used for sandbox downloads. Falls back to the
    /// public endpoint when unset.
    pub internal_endpoint: Option<String>,
    pub access_key_id: Option<String>,
    pub access_key_secret: Option<SecretString>,
    pub key_prefix: String,
}

impl Default for OssConfig {
    fn default() -> Self {
        Self {
            bucket: None,
            region: None,
            public_endpoint: None,
            internal_endpoint: None,
            access_key_id: None,
            access_key_secret: None,
            key_prefix: DEFAULT_KEY_PREFIX.into(),
        }
    }
}

impl Debug for OssConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OssConfig")
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("public_endpoint", &self.public_endpoint)
            .field("internal_endpoint", &self.internal_endpoint)
            .field(
                "access_key_id",
                &self.access_key_id.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "access_key_secret",
                &self.access_key_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("key_prefix", &self.key_prefix)
            .finish()
    }
}

impl OssConfig {
    pub fn from_env() -> Self {
        let bucket = non_empty_env("QUIRE_OSS_BUCKET");
        let region = non_empty_env("QUIRE_OSS_REGION");
        let public_endpoint = non_empty_env("QUIRE_OSS_PUBLIC_ENDPOINT");
        let internal_endpoint = non_empty_env("QUIRE_OSS_INTERNAL_ENDPOINT");
        let access_key_id =
            non_empty_env("OSS_ACCESS_KEY_ID").or_else(|| non_empty_env("QUIRE_OSS_ACCESS_KEY_ID"));
        let access_key_secret = env::var("OSS_ACCESS_KEY_SECRET")
            .ok()
            .or_else(|| env::var("QUIRE_OSS_ACCESS_KEY_SECRET").ok())
            .filter(|value| !value.trim().is_empty())
            .map(SecretString::from);
        let key_prefix = non_empty_env("QUIRE_OSS_KEY_PREFIX")
            .map(|value| normalize_prefix(&value))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_KEY_PREFIX.into());
        Self {
            bucket,
            region,
            public_endpoint,
            internal_endpoint,
            access_key_id,
            access_key_secret,
            key_prefix,
        }
    }

    pub fn is_configured(&self) -> bool {
        self.bucket
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            && self
                .region
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            && self
                .public_endpoint
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            && self
                .access_key_id
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            && self
                .access_key_secret
                .as_ref()
                .is_some_and(|value| !value.expose_secret().trim().is_empty())
    }

    /// Prefix shared by every attachment of one session.
    pub fn object_prefix(&self, session_id: &str) -> String {
        let prefix = normalize_prefix(&self.key_prefix);
        let prefix = if prefix.is_empty() {
            DEFAULT_KEY_PREFIX
        } else {
            prefix.as_str()
        };
        format!("{prefix}/{session_id}")
    }

    pub fn object_key(&self, session_id: &str, attachment_id: &str, filename: &str) -> String {
        format!(
            "{}/{attachment_id}/{filename}",
            self.object_prefix(session_id)
        )
    }

    /// Check that a stored object key belongs to one session and has no
    /// characters that could confuse URL or shell handling.
    pub fn owns_object_key(&self, session_id: &str, object_key: &str) -> bool {
        let prefix = format!("{}/", self.object_prefix(session_id));
        object_key.starts_with(&prefix)
            && object_key.len() > prefix.len()
            && !object_key.contains('?')
            && !object_key.contains('#')
            && !object_key.chars().any(char::is_control)
    }

    /// Presign a browser-facing PUT URL. The Content-Type header is returned to
    /// the browser by the API but intentionally excluded from the signature so
    /// the browser can send the exact file type without header canonicalization.
    pub fn presign_put(
        &self,
        object_key: &str,
        content_type: &str,
        ttl_secs: u64,
    ) -> RuntimeResult<String> {
        if content_type.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "attachment content type must not be empty".into(),
            ));
        }
        self.presign_at(
            "PUT",
            self.public_endpoint.as_deref(),
            object_key,
            Some(content_type.trim()),
            ttl_secs,
            Utc::now(),
        )
    }

    /// Whether to verify completed uploads against the live OSS endpoint.
    /// Disabled by default so tests and development fixtures never make unexpected network calls.
    pub fn is_live_endpoint(&self) -> bool {
        env::var("QUIRE_OSS_VERIFY_UPLOADS")
            .ok()
            .is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
    }

    /// Presign a HEAD request used to verify an uploaded object's existence and size.
    pub fn presign_head(&self, object_key: &str, ttl_secs: u64) -> RuntimeResult<String> {
        let endpoint = self
            .internal_endpoint
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(self.public_endpoint.as_deref());
        self.presign_at("HEAD", endpoint, object_key, None, ttl_secs, Utc::now())
    }

    /// Presign a DELETE request used to remove an uploaded object when deleting a session.
    pub fn presign_delete(&self, object_key: &str, ttl_secs: u64) -> RuntimeResult<String> {
        let endpoint = self
            .internal_endpoint
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(self.public_endpoint.as_deref());
        self.presign_at("DELETE", endpoint, object_key, None, ttl_secs, Utc::now())
    }

    /// Presign a sandbox-facing GET URL, preferring the VPC endpoint.
    pub fn presign_get_for_sandbox(
        &self,
        object_key: &str,
        ttl_secs: u64,
    ) -> RuntimeResult<String> {
        let endpoint = self
            .internal_endpoint
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(self.public_endpoint.as_deref());
        self.presign_at("GET", endpoint, object_key, None, ttl_secs, Utc::now())
    }

    fn presign_at(
        &self,
        method: &str,
        endpoint: Option<&str>,
        object_key: &str,
        content_type: Option<&str>,
        ttl_secs: u64,
        now: DateTime<Utc>,
    ) -> RuntimeResult<String> {
        if !self.is_configured() {
            return Err(RuntimeError::InvalidRequest(
                "Alibaba Cloud OSS is not configured".into(),
            ));
        }
        if ttl_secs == 0 || ttl_secs > MAX_EXPIRES_SECS {
            return Err(RuntimeError::InvalidRequest(format!(
                "OSS URL expiry must be between 1 and {MAX_EXPIRES_SECS} seconds"
            )));
        }
        if object_key.trim().is_empty()
            || object_key.starts_with('/')
            || object_key.contains('?')
            || object_key.contains('#')
        {
            return Err(RuntimeError::InvalidRequest(
                "OSS object key is invalid".into(),
            ));
        }
        let endpoint = endpoint
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| RuntimeError::InvalidRequest("OSS endpoint is not configured".into()))?;
        let bucket = self
            .bucket
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| RuntimeError::InvalidRequest("OSS bucket is not configured".into()))?;
        let region = self
            .region
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| RuntimeError::InvalidRequest("OSS region is not configured".into()))?;
        let access_key_id = self
            .access_key_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                RuntimeError::InvalidRequest("OSS AccessKey ID is not configured".into())
            })?;
        let access_key_secret = self
            .access_key_secret
            .as_ref()
            .ok_or_else(|| {
                RuntimeError::InvalidRequest("OSS AccessKey secret is not configured".into())
            })?
            .expose_secret();

        let endpoint = Endpoint::parse(endpoint, bucket)?;
        let date_time = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let scope = format!("{date}/{region}/{SERVICE}/{REQUEST_SUFFIX}");
        let credential = format!("{access_key_id}/{scope}");

        let mut parameters = [
            ("x-oss-credential", credential),
            ("x-oss-date", date_time.clone()),
            ("x-oss-expires", ttl_secs.to_string()),
            ("x-oss-signature-version", ALGORITHM.to_string()),
        ];
        parameters.sort_by(|left, right| left.0.cmp(right.0));
        let canonical_query = parameters
            .iter()
            .map(|(key, value)| format!("{}={}", uri_encode(key, false), uri_encode(value, false)))
            .collect::<Vec<_>>()
            .join("&");

        let encoded_key = uri_encode(object_key, true);
        let canonical_uri = format!("/{}/{}", uri_encode(bucket, false), encoded_key);
        // OSS only includes Content-Type and Content-MD5 automatically in the
        // canonical headers. `host` participates only when explicitly listed
        // through x-oss-additional-headers, which these URLs do not use.
        let canonical_headers = content_type
            .map(|content_type| format!("content-type:{}\n", content_type.trim()))
            .unwrap_or_default();
        let canonical_request = [
            method,
            canonical_uri.as_str(),
            canonical_query.as_str(),
            canonical_headers.as_str(),
            "",
            UNSIGNED_PAYLOAD,
        ]
        .join("\n");
        let string_to_sign = format!(
            "{ALGORITHM}\n{date_time}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );
        let signature = hex(&hmac_sha256(
            &signing_key(access_key_secret, &date, region),
            string_to_sign.as_bytes(),
        ));

        Ok(format!(
            "{}://{}/{encoded_key}?{canonical_query}&x-oss-signature={signature}",
            endpoint.scheme, endpoint.host_header
        ))
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_prefix(value: &str) -> String {
    value.trim().trim_matches('/').to_string()
}

struct Endpoint {
    scheme: String,
    host_header: String,
}

impl Endpoint {
    fn parse(endpoint: &str, bucket: &str) -> RuntimeResult<Self> {
        let url = reqwest::Url::parse(endpoint)
            .map_err(|_| RuntimeError::InvalidRequest("OSS endpoint URL is invalid".into()))?;
        let scheme = url.scheme();
        if scheme != "https" && scheme != "http" {
            return Err(RuntimeError::InvalidRequest(
                "OSS endpoint must use http or https".into(),
            ));
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(RuntimeError::InvalidRequest(
                "OSS endpoint must not contain a query or fragment".into(),
            ));
        }
        if !matches!(url.path(), "" | "/") {
            return Err(RuntimeError::InvalidRequest(
                "OSS endpoint must not contain a path".into(),
            ));
        }
        let host = url
            .host_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| RuntimeError::InvalidRequest("OSS endpoint has no host".into()))?;
        let bucket_prefix = format!("{bucket}.");
        let host = if host.starts_with(&bucket_prefix) {
            host.to_string()
        } else {
            format!("{bucket_prefix}{host}")
        };
        let host_header = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host,
        };
        Ok(Self {
            scheme: scheme.to_string(),
            host_header,
        })
    }
}

fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let initial = format!("aliyun_v4{secret}");
    let date_key = hmac_sha256(initial.as_bytes(), date.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, SERVICE.as_bytes());
    hmac_sha256(&service_key, REQUEST_SUFFIX.as_bytes())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&key, data).as_ref().to_vec()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, data).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn uri_encode(value: &str, keep_slashes: bool) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let byte = *byte;
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (keep_slashes && byte == b'/')
        {
            output.push(char::from(byte));
        } else {
            let _ = write!(output, "%{byte:02X}");
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn configured(internal: Option<&str>) -> OssConfig {
        OssConfig {
            bucket: Some("agentcore-ws-test".into()),
            region: Some("cn-hangzhou".into()),
            public_endpoint: Some("https://oss-cn-hangzhou.aliyuncs.com".into()),
            internal_endpoint: internal.map(str::to_string),
            access_key_id: Some("LTAI_TEST_ACCESS_KEY".into()),
            access_key_secret: Some(SecretString::from("test-secret-value")),
            key_prefix: "quire/attachments".into(),
        }
    }

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 20, 8, 0, 0)
            .single()
            .unwrap_or_else(|| DateTime::<Utc>::from(std::time::UNIX_EPOCH))
    }

    #[test]
    fn put_presign_matches_official_v4_vector() -> RuntimeResult<()> {
        let config = configured(None);
        let object_key = "quire/attachments/sess_test/att_test/data.csv";
        let url = config.presign_at(
            "PUT",
            config.public_endpoint.as_deref(),
            object_key,
            Some("text/csv"),
            900,
            fixed_now(),
        )?;
        assert_eq!(
            url,
            "https://agentcore-ws-test.oss-cn-hangzhou.aliyuncs.com/quire/attachments/sess_test/att_test/data.csv?x-oss-credential=LTAI_TEST_ACCESS_KEY%2F20261120%2Fcn-hangzhou%2Foss%2Faliyun_v4_request&x-oss-date=20261120T080000Z&x-oss-expires=900&x-oss-signature-version=OSS4-HMAC-SHA256&x-oss-signature=d9e0145ed914a4da2ccfd507175a6819894e5f16bacc2cdbf465b993f78a7a6b"
        );
        Ok(())
    }

    #[test]
    fn sandbox_get_prefers_internal_endpoint_and_falls_back() -> RuntimeResult<()> {
        let internal = configured(Some("https://oss-cn-hangzhou-internal.aliyuncs.com"));
        let url = internal.presign_at(
            "GET",
            internal.internal_endpoint.as_deref(),
            "quire/attachments/sess/att/file.txt",
            None,
            600,
            fixed_now(),
        )?;
        assert!(
            url.starts_with("https://agentcore-ws-test.oss-cn-hangzhou-internal.aliyuncs.com/")
        );
        assert!(url.contains("&x-oss-signature="));

        let public = configured(None);
        let url = public.presign_at(
            "GET",
            public
                .internal_endpoint
                .as_deref()
                .or(public.public_endpoint.as_deref()),
            "quire/attachments/sess/att/file.txt",
            None,
            600,
            fixed_now(),
        )?;
        assert!(url.starts_with("https://agentcore-ws-test.oss-cn-hangzhou.aliyuncs.com/"));
        Ok(())
    }

    #[test]
    fn object_keys_are_session_scoped() {
        let config = configured(None);
        assert_eq!(
            config.object_key("sess_1", "att_2", "data.csv"),
            "quire/attachments/sess_1/att_2/data.csv"
        );
        assert_eq!(config.object_prefix("sess_1"), "quire/attachments/sess_1");
    }

    #[test]
    fn debug_never_prints_credentials() {
        let config = configured(None);
        let rendered = format!("{config:?}");
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("LTAI_TEST_ACCESS_KEY"));
        assert!(!rendered.contains("test-secret-value"));
    }

    #[test]
    fn unconfigured_or_invalid_input_fails_closed() {
        assert!(!OssConfig::default().is_configured());
        assert!(
            OssConfig::default()
                .presign_get_for_sandbox("key", 600)
                .is_err()
        );
        assert!(configured(None).presign_get_for_sandbox("key", 0).is_err());
        assert!(configured(None).presign_put("key", " ", 600).is_err());
        assert!(
            configured(None)
                .presign_put("/key", "text/plain", 600)
                .is_err()
        );
    }
}
