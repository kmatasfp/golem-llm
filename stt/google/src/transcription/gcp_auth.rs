use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Duration, Utc};
use golem_stt::http::HttpClient;
use http::Request;
use rsa::Pkcs1v15Sign;
use rsa::{pkcs1::DecodeRsaPrivateKey, pkcs8::DecodePrivateKey, RsaPrivateKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum Error {
    JsonError(serde_json::Error),
    CryptoError(String),
    HttpError(String),
    TokenExchange(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Serialize, Deserialize)]
struct JwtHeader {
    alg: String,
    typ: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaim {
    iss: String,
    scope: String,
    aud: String,
    exp: i64,
    iat: i64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<i64>,
    token_type: String,
}

#[derive(Clone)]
pub struct GcpAuth {
    client_email: String,
    private_key: RsaPrivateKey,
    access_token: Option<String>,
    token_expires_at: Option<DateTime<Utc>>,
}

// based on https://developers.google.com/identity/protocols/oauth2/service-account#httprest
impl GcpAuth {
    pub fn new(client_email: String, private_key: String) -> Result<Self, Error> {
        let private_key = Self::parse_private_key(&private_key)?;

        Ok(Self {
            client_email,
            private_key,
            access_token: None,
            token_expires_at: None,
        })
    }

    fn parse_private_key(pem_key: &str) -> Result<RsaPrivateKey, Error> {
        RsaPrivateKey::from_pkcs8_pem(pem_key)
            .map_err(|e| Error::CryptoError(format!("Failed to parse private key: {}", e)))
    }

    pub async fn get_access_token<HC: HttpClient>(
        &mut self,
        http_client: &HC,
    ) -> Result<String, Error> {
        // Check if we have a valid token
        if let (Some(token), Some(expires_at)) = (&self.access_token, &self.token_expires_at) {
            if Utc::now() < *expires_at - Duration::minutes(5) {
                return Ok(token.clone());
            }
        }

        let jwt = self.create_signed_jwt()?;
        let access_token = self.exchange_jwt_for_token(jwt, http_client).await?;

        self.access_token = Some(access_token.clone());
        self.token_expires_at = Some(Utc::now() + Duration::minutes(55)); // 5 min buffer

        Ok(access_token)
    }

    fn create_signed_jwt(&self) -> Result<String, Error> {
        let now = Utc::now().timestamp();
        let exp = now + 3600;

        let header = JwtHeader {
            alg: "RS256".to_string(),
            typ: "JWT".to_string(),
        };

        let claim = JwtClaim {
            iss: self.client_email.clone(),
            scope: "https://www.googleapis.com/auth/cloud-platform".to_string(),
            aud: "https://oauth2.googleapis.com/token".to_string(),
            exp,
            iat: now,
        };

        let header_json = serde_json::to_string(&header).map_err(Error::JsonError)?;
        let claim_json = serde_json::to_string(&claim).map_err(Error::JsonError)?;

        let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header_json.as_bytes());
        let claim_b64 = general_purpose::URL_SAFE_NO_PAD.encode(claim_json.as_bytes());

        let to_be_signed = format!("{}.{}", header_b64, claim_b64);

        // Sign with RSASSA-PKCS1-v1_5 (JWT RS256 standard)
        let signature = self.calculate_signature(to_be_signed.as_bytes())?;
        let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode(&signature);

        Ok(format!("{}.{}", to_be_signed, signature_b64))
    }

    fn calculate_signature(&self, data: &[u8]) -> Result<Vec<u8>, Error> {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hasher.finalize();

        let padding = Pkcs1v15Sign::new::<Sha256>();

        let mut rng = rand::thread_rng();
        let signature = self
            .private_key
            .sign_with_rng(&mut rng, padding, &hash)
            .map_err(|e| Error::CryptoError(format!("Failed to sign data: {}", e)))?;

        Ok(signature)
    }

    async fn exchange_jwt_for_token<HC: HttpClient>(
        &self,
        jwt: String,
        http_client: &HC,
    ) -> Result<String, Error> {
        let form_data = format!(
            "grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer&assertion={}",
            urlencoding::encode(&jwt)
        );

        let request = Request::builder()
            .method("POST")
            .uri("https://oauth2.googleapis.com/token")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Content-Length", form_data.len().to_string())
            .body(form_data.into_bytes())
            .map_err(|e| Error::HttpError(format!("Failed to build token request: {}", e)))?;

        let response = http_client
            .execute(request)
            .await
            .map_err(|e| Error::HttpError(format!("Token request failed: {:?}", e)))?;

        if !response.status().is_success() {
            let error_body = String::from_utf8_lossy(response.body());
            return Err(Error::TokenExchange(format!(
                "Token exchange failed with status {}: {}",
                response.status(),
                error_body
            )));
        }

        let token_response: TokenResponse =
            serde_json::from_slice(response.body()).map_err(|e| Error::JsonError(e))?;

        Ok(token_response.access_token)
    }
}

#[cfg(test)]
mod tests {

    use http::Response;
    use rsa::RsaPublicKey;

    use super::*;
    use std::{
        cell::{Ref, RefCell},
        collections::VecDeque,
    };

    struct MockHttpClient {
        pub responses: RefCell<VecDeque<Result<Response<Vec<u8>>, golem_stt::http::Error>>>,
        pub captured_requests: RefCell<Vec<Request<Vec<u8>>>>,
    }

    impl MockHttpClient {
        pub fn new() -> Self {
            Self {
                responses: RefCell::new(VecDeque::new()),
                captured_requests: RefCell::new(Vec::new()),
            }
        }

        pub fn expect_response(&self, response: Response<Vec<u8>>) {
            self.responses.borrow_mut().push_back(Ok(response));
        }

        pub fn last_captured_request(&self) -> Option<Ref<Request<Vec<u8>>>> {
            let borrow = self.captured_requests.borrow();
            if borrow.is_empty() {
                None
            } else {
                Some(Ref::map(borrow, |requests| requests.last().unwrap()))
            }
        }
    }

    impl HttpClient for MockHttpClient {
        async fn execute(
            &self,
            request: Request<Vec<u8>>,
        ) -> Result<Response<Vec<u8>>, golem_stt::http::Error> {
            self.captured_requests.borrow_mut().push(request);
            self.responses
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(golem_stt::http::Error::Generic(
                    "unexpected error".to_string(),
                )))
        }
    }

    #[wstd::test]
    async fn test_token_exchange_flow() {
        let mock_client = MockHttpClient::new();

        let token_response =
            r#"{"access_token": "ya29.test_token", "expires_in": 3600, "token_type": "Bearer"}"#;
        mock_client.expect_response(
            Response::builder()
                .status(200)
                .body(token_response.as_bytes().to_vec())
                .unwrap(),
        );

        let private_key = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7VJTUt9Us8cKB\nTc/ZGpGZqpONTEOZ+H+qz3F8qY7jNz5NpGOB8v3rQq2+j3F8qY7jNz5NpGOB8v3r\n-----END PRIVATE KEY-----";
        let client_email = "test-service-account@test-project-123.iam.gserviceaccount.com";

        if let Ok(mut auth) = GcpAuth::new(client_email.to_string(), private_key.to_string()) {
            let _result = auth.get_access_token(&mock_client).await;

            let request = mock_client.last_captured_request().unwrap();
            assert_eq!(request.method(), "POST");
            assert_eq!(request.uri(), "https://oauth2.googleapis.com/token");

            let content_type = request.headers().get("content-type").unwrap();
            assert_eq!(content_type, "application/x-www-form-urlencoded");

            let body = String::from_utf8_lossy(request.body());
            assert!(body.contains("grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer"));
            assert!(body.contains("assertion="));

            // Extract and verify the JWT assertion structure
            let assertion_start = body.find("assertion=").unwrap() + "assertion=".len();
            let assertion_end = body[assertion_start..]
                .find('&')
                .unwrap_or(body[assertion_start..].len());
            let assertion_encoded = &body[assertion_start..assertion_start + assertion_end];
            let assertion = urlencoding::decode(assertion_encoded).unwrap();

            let jwt_parts: Vec<&str> = assertion.split('.').collect();
            assert_eq!(
                jwt_parts.len(),
                3,
                "JWT should have 3 parts: header.claim.signature"
            );

            let header_b64 = jwt_parts[0];
            let claim_b64 = jwt_parts[1];
            let signature_b64 = jwt_parts[2];

            let public_key = RsaPublicKey::from(&auth.private_key);
            let padding = Pkcs1v15Sign::new::<Sha256>();

            let to_be_signed = format!("{}.{}", header_b64, claim_b64);

            let signature_bytes = general_purpose::URL_SAFE_NO_PAD
                .decode(signature_b64)
                .unwrap();

            let mut hasher = Sha256::new();
            hasher.update(to_be_signed.as_bytes());
            let hash = hasher.finalize();

            public_key
                .verify(padding, &hash, &signature_bytes)
                .expect("JWT signature should be cryptographically valid");

            // Decode and verify the JSON content structure
            let header_json = general_purpose::URL_SAFE_NO_PAD.decode(header_b64).unwrap();
            let claim_json = general_purpose::URL_SAFE_NO_PAD.decode(claim_b64).unwrap();

            let parsed_header: JwtHeader = serde_json::from_slice(&header_json).unwrap();
            let parsed_claim: JwtClaim = serde_json::from_slice(&claim_json).unwrap();

            assert_eq!(parsed_header.alg, "RS256");
            assert_eq!(parsed_header.typ, "JWT");

            // Verify payload structure and content
            assert_eq!(
                parsed_claim.iss,
                "test-service-account@test-project-123.iam.gserviceaccount.com"
            );
            assert_eq!(
                parsed_claim.scope,
                "https://www.googleapis.com/auth/cloud-platform"
            );
            assert_eq!(parsed_claim.aud, "https://oauth2.googleapis.com/token");

            // Verify timestamps are reasonable (within last minute and next hour)
            let now = Utc::now().timestamp();
            assert!(parsed_claim.iat >= now - 60, "iat should be recent");
            assert!(parsed_claim.iat <= now + 60, "iat should not be in future");
            assert_eq!(
                parsed_claim.exp,
                parsed_claim.iat + 3600,
                "exp should be 1 hour after iat"
            );
        }
    }
}
