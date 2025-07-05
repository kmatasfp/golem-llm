use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use http::{HeaderMap, HeaderValue, Request};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use sha2::{Digest, Sha256};
use std::fmt;

use crate::error::Error;

type HmacSha256 = Hmac<Sha256>;

pub struct AwsSignatureV4 {
    access_key: String,
    secret_key: String,
    region: String,
    service: String,
}

pub enum AwsService {
    S3,
    Transcribe,
}

impl fmt::Display for AwsService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AwsService::S3 => write!(f, "s3"),
            AwsService::Transcribe => write!(f, "transcribe"),
        }
    }
}

// Percent-encoding set for URI paths
// Why this is needed see here https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_sigv-create-signed-request.html
// AWS uri encoding has special characters that need to be percent-encoded
const URI_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'?')
    .add(b'{')
    .add(b'}');

/// AWS-specific percent-encoding set for query strings
const QUERY_ENCODE_SET: &AsciiSet = &URI_ENCODE_SET.add(b'=').add(b'&').add(b'+');

impl AwsSignatureV4 {
    pub fn new(
        access_key: String,
        secret_key: String,
        region: String,
        service: AwsService,
    ) -> Self {
        Self {
            access_key,
            secret_key,
            region,
            service: service.to_string(),
        }
    }

    /// Create a signer for Amazon S3
    pub fn for_s3(access_key: String, secret_key: String, region: String) -> Self {
        Self::new(access_key, secret_key, region, AwsService::S3)
    }

    pub fn for_transcribe(access_key: String, secret_key: String, region: String) -> Self {
        Self::new(access_key, secret_key, region, AwsService::Transcribe)
    }

    pub fn sign_request<T>(
        &self,
        request: Request<T>,
        timestamp: DateTime<Utc>,
    ) -> Result<Request<T>, Error>
    where
        T: AsRef<[u8]>,
    {
        let (mut parts, body) = request.into_parts();

        let date_stamp = timestamp.format("%Y%m%d").to_string();
        let amz_date = timestamp.format("%Y%m%dT%H%M%SZ").to_string();

        parts
            .headers
            .insert("x-amz-date", HeaderValue::from_str(&amz_date)?);

        // let content_sha256 = self.hash_payload(body.as_ref());
        // parts.headers.insert(
        //     "x-amz-content-sha256",
        //     HeaderValue::from_str(&content_sha256)?,
        // );

        // Add host header if not present
        if !parts.headers.contains_key("host") {
            if let Some(host) = parts.uri.host() {
                let host_header = if let Some(port) = parts.uri.port_u16() {
                    format!("{}:{}", host, port)
                } else {
                    host.to_string()
                };
                parts
                    .headers
                    .insert("host", HeaderValue::from_str(&host_header)?);
            }
        }

        let canonical_request =
            self.create_canonical_request(&parts.method, &parts.uri, &parts.headers, body.as_ref());

        let string_to_sign = self.create_string_to_sign(&canonical_request, &amz_date, &date_stamp);

        let signature = self.calculate_signature(&string_to_sign, &date_stamp)?;

        let signed_headers = self.get_signed_headers(&parts.headers);
        let credential = format!(
            "{}/{}/{}/{}/aws4_request",
            self.access_key, date_stamp, self.region, self.service
        );
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}, SignedHeaders={}, Signature={}",
            credential, signed_headers, signature
        );

        parts
            .headers
            .insert("authorization", HeaderValue::from_str(&authorization)?);

        Ok(Request::from_parts(parts, body))
    }

    fn create_canonical_request(
        &self,
        method: &http::Method,
        uri: &http::Uri,
        headers: &HeaderMap,
        body: &[u8],
    ) -> String {
        // Canonical URI
        let canonical_uri = self.canonical_uri(uri.path());

        // Canonical query string
        let canonical_query_string = self.canonical_query_string(uri.query().unwrap_or(""));

        // Canonical headers
        let canonical_headers = self.canonical_headers(headers);

        // Signed headers
        let signed_headers = self.get_signed_headers(headers);

        // Hashed payload
        let hashed_payload = self.hash_payload(body);

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method.as_str().to_uppercase(),
            canonical_uri,
            canonical_query_string,
            canonical_headers,
            signed_headers,
            hashed_payload
        );

        canonical_request
    }

    fn canonical_uri(&self, path: &str) -> String {
        if path.is_empty() {
            "/".to_string()
        } else {
            // URI encode each segment
            let segments: Vec<String> = path
                .split('/')
                .map(|segment| utf8_percent_encode(segment, URI_ENCODE_SET).to_string())
                .collect();
            segments.join("/")
        }
    }

    fn canonical_query_string(&self, query: &str) -> String {
        if query.is_empty() {
            return String::new();
        }

        let mut params: Vec<(String, String)> = query
            .split('&')
            .filter_map(|param| {
                if let Some(eq_pos) = param.find('=') {
                    let key = &param[..eq_pos];
                    let value = &param[eq_pos + 1..];
                    Some((
                        utf8_percent_encode(key, QUERY_ENCODE_SET).to_string(),
                        utf8_percent_encode(value, QUERY_ENCODE_SET).to_string(),
                    ))
                } else {
                    Some((
                        utf8_percent_encode(param, QUERY_ENCODE_SET).to_string(),
                        String::new(),
                    ))
                }
            })
            .collect();

        params.sort_by(|a, b| a.0.cmp(&b.0));

        params
            .into_iter()
            .map(|(key, value)| {
                if value.is_empty() {
                    key
                } else {
                    format!("{}={}", key, value)
                }
            })
            .collect::<Vec<_>>()
            .join("&")
    }

    fn canonical_headers(&self, headers: &HeaderMap) -> String {
        let mut canonical_headers = String::new();

        // Collect and sort headers
        let mut sorted_headers: Vec<_> = headers
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_lowercase(),
                    value.to_str().unwrap_or("").trim(),
                )
            })
            .collect();
        sorted_headers.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, value) in sorted_headers {
            canonical_headers.push_str(&format!("{}:{}\n", name, value));
        }

        canonical_headers
    }

    fn get_signed_headers(&self, headers: &HeaderMap) -> String {
        let mut signed_headers: Vec<String> = headers
            .keys()
            .map(|key| key.as_str().to_lowercase())
            .collect();
        signed_headers.sort();
        signed_headers.join(";")
    }

    fn hash_payload(&self, payload: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(payload);
        hex::encode(hasher.finalize())
    }

    fn create_string_to_sign(
        &self,
        canonical_request: &str,
        amz_date: &str,
        date_stamp: &str,
    ) -> String {
        let algorithm = "AWS4-HMAC-SHA256";
        let credential_scope = format!(
            "{}/{}/{}/aws4_request",
            date_stamp, self.region, self.service
        );

        let mut hasher = Sha256::new();
        hasher.update(canonical_request.as_bytes());
        let hashed_canonical_request = hex::encode(hasher.finalize());

        let string_to_sign = format!(
            "{}\n{}\n{}\n{}",
            algorithm, amz_date, credential_scope, hashed_canonical_request
        );

        string_to_sign
    }

    fn calculate_signature(&self, string_to_sign: &str, date_stamp: &str) -> Result<String, Error> {
        let secret = format!("AWS4{}", self.secret_key);

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
        mac.update(date_stamp.as_bytes());
        let date_key = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&date_key)?;
        mac.update(self.region.as_bytes());
        let date_region_key = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&date_region_key)?;
        mac.update(self.service.as_bytes());
        let date_region_service_key = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&date_region_service_key)?;
        mac.update(b"aws4_request");
        let signing_key = mac.finalize().into_bytes();

        let mut mac = HmacSha256::new_from_slice(&signing_key)?;
        mac.update(string_to_sign.as_bytes());
        let signature = mac.finalize().into_bytes();

        Ok(hex::encode(signature))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_credential_types::Credentials;
    use http::{Method, Request};

    use aws_sigv4::{
        http_request::{sign, SignableBody, SignableRequest, SigningSettings},
        sign::v4,
    };

    fn sign_with_aws_sdk<T>(
        mut request: Request<T>,
        access_key: &str,
        secret_key: &str,
        region: &str,
        service: &str,
        timestamp: DateTime<Utc>,
    ) -> Request<T>
    where
        T: AsRef<[u8]>,
    {
        let creds = Credentials::new(access_key, secret_key, None, None, "iam");
        let identity = creds.into();

        let signing_settings = SigningSettings::default();
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&region)
            .name(service)
            .time(timestamp.into())
            .settings(signing_settings)
            .build()
            .unwrap()
            .into();

        let signable_request = SignableRequest::new(
            request.method().as_str(),
            request.uri().to_string(),
            request
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str(), std::str::from_utf8(v.as_bytes()).unwrap())),
            SignableBody::Bytes(request.body().as_ref()),
        )
        .unwrap();

        let (signing_instructions, _signature) = sign(signable_request, &signing_params)
            .unwrap()
            .into_parts();
        signing_instructions.apply_to_request_http1x(&mut request);

        request
    }

    // test agains examples in https://docs.aws.amazon.com/AmazonS3/latest/API/sig-v4-header-based-auth.html
    #[test]
    fn test_s3_get_object_authorization_header() {
        // Test case from AWS documentation
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let signer = AwsSignatureV4::for_s3(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
        );

        let request = Request::builder()
            .method(Method::GET)
            .uri("s3://examplebucket.s3.amazonaws.com/test.txt")
            .header("Range", "bytes=0-9")
            .body(vec![])
            .unwrap();

        let timestamp = DateTime::parse_from_rfc2822("Fri, 24 May 2013 00:00:00 GMT")
            .unwrap()
            .with_timezone(&Utc);

        let result = signer.sign_request(request, timestamp);
        assert!(result.is_ok(), "Failed to sign request: {:?}", result.err());

        let signed_request = result.unwrap();

        let auth_header = signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();
        let expected_auth = "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders=host;range;x-amz-content-sha256;x-amz-date,Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41";

        assert_eq!(auth_header, expected_auth, "Authorization header mismatch");
    }

    #[test]
    fn test_s3_put_object_authorization_header() {
        // Test case from AWS documentation
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let signer = AwsSignatureV4::for_s3(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
        );

        // Create the exact request from your specification
        let request = Request::builder()
            .method(Method::PUT)
            .uri("s3://examplebucket.s3.amazonaws.com/test$file.text")
            .header("Date", "Fri, 24 May 2013 00:00:00 GMT")
            .header("x-amz-storage-class", "REDUCED_REDUNDANCY")
            .body(b"Welcome to Amazon S3.".to_vec())
            .unwrap();

        // Parse the Date header directly: "Fri, 24 May 2013 00:00:00 GMT"
        let timestamp = DateTime::parse_from_rfc2822("Fri, 24 May 2013 00:00:00 GMT")
            .unwrap()
            .with_timezone(&Utc);

        let result = signer.sign_request(request, timestamp);
        assert!(result.is_ok(), "Failed to sign request: {:?}", result.err());

        let signed_request = result.unwrap();

        // Get the authorization header
        let auth_header = signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        let expected_auth = "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders=date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class,Signature=98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd";
        assert_eq!(auth_header, expected_auth, "Authorization header mismatch");
    }

    #[test]
    fn test_s3_list_objects_authorization_header() {
        // Test case from AWS documentation
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let signer = AwsSignatureV4::for_s3(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
        );

        // Create the exact request from your specification
        let request = Request::builder()
            .method(Method::GET)
            .uri("s3://examplebucket.s3.amazonaws.com/?max-keys=2&prefix=J")
            .body(vec![])
            .unwrap();

        // Parse the Date header directly: "Fri, 24 May 2013 00:00:00 GMT"
        let timestamp = DateTime::parse_from_rfc2822("Fri, 24 May 2013 00:00:00 GMT")
            .unwrap()
            .with_timezone(&Utc);

        let result = signer.sign_request(request, timestamp);
        assert!(result.is_ok(), "Failed to sign request: {:?}", result.err());

        let signed_request = result.unwrap();

        // Get the authorization header
        let auth_header = signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        let expected_auth = "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,SignedHeaders=host;x-amz-content-sha256;x-amz-date,Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7";
        assert_eq!(auth_header, expected_auth, "Authorization header mismatch");
    }

    #[test]
    fn test_batch_transcription_authorization_header() {
        // Test case from AWS documentation
        let access_key = "AKIAIOSFODNN7EXAMPLE";
        let secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let region = "us-east-1";

        let signer = AwsSignatureV4::for_transcribe(
            access_key.to_string(),
            secret_key.to_string(),
            region.to_string(),
        );

        let body = r#"
            {
                "TranscriptionJobName": "my-first-transcription-job",
                "LanguageCode": "en-US",
                "Media": {
                    "MediaFileUri": "s3://amzn-s3-demo-bucket/my-input-files/my-media-file.flac"
                },
                "OutputBucketName": "amzn-s3-demo-bucket",
                "OutputKey": "my-output-files/"
            }
            "#;

        // Create the exact request from your specification
        let request = Request::builder()
            .method(Method::GET)
            .uri("s3://examplebucket.s3.amazonaws.com/?max-keys=2&prefix=J")
            .body(body.as_bytes())
            .unwrap();

        let request_for_aws_sdk = request.clone();

        // Parse the Date header directly: "Fri, 24 May 2013 00:00:00 GMT"
        let timestamp = DateTime::parse_from_rfc2822("Fri, 24 May 2013 00:00:00 GMT")
            .unwrap()
            .with_timezone(&Utc);

        let result = signer.sign_request(request, timestamp);
        assert!(result.is_ok(), "Failed to sign request: {:?}", result.err());

        let signed_request = result.unwrap();

        // Get the authorization header
        let auth_header = signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        let aws_signed_request = sign_with_aws_sdk(
            request_for_aws_sdk,
            access_key,
            secret_key,
            region,
            "transcribe",
            timestamp,
        );

        let expected_auth = aws_signed_request
            .headers()
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap();

        assert_eq!(auth_header, expected_auth, "Authorization header mismatch");
    }
}
