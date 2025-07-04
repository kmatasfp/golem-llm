use std::collections::VecDeque;

use crate::client::SttProviderClient;

#[allow(unused)]
pub struct TranscriptionQueue<'a, API, REQ, RES, ERR>
where
    API: SttProviderClient<REQ, RES, ERR>,
    ERR: std::error::Error,
{
    api: &'a API,
    requests: VecDeque<REQ>,
    processed_results: VecDeque<Result<RES, ERR>>,
}

#[allow(unused)]
impl<'a, API, REQ, RES, ERR> TranscriptionQueue<'a, API, REQ, RES, ERR>
where
    API: SttProviderClient<REQ, RES, ERR>,
    ERR: std::error::Error,
{
    pub fn new(api: &'a API, requests: Vec<REQ>) -> Self {
        Self {
            api,
            requests: requests.into(),
            processed_results: VecDeque::new(),
        }
    }

    fn process_next(&mut self) -> bool {
        if let Some(request) = self.requests.pop_front() {
            let result = self.api.transcribe_audio(request);
            self.processed_results.push_back(result);
            true
        } else {
            false
        }
    }

    pub fn get_next(&mut self) -> Option<Result<RES, ERR>> {
        if self.processed_results.is_empty() && !self.requests.is_empty() {
            self.process_next();
        }

        self.processed_results.pop_front()
    }

    pub fn blocking_get_next(&mut self) -> Vec<Result<RES, ERR>> {
        while !self.requests.is_empty() {
            self.process_next();
        }

        let mut results = Vec::new();
        while let Some(result) = self.processed_results.pop_front() {
            results.push(result);
        }

        results
    }

    pub fn has_pending(&self) -> bool {
        !self.requests.is_empty() || !self.processed_results.is_empty()
    }

    pub fn nr_remaining_requests(&self) -> usize {
        self.requests.len()
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque};

    use crate::{client::SttProviderClient, transcription_queue::TranscriptionQueue};

    #[derive(Debug, PartialEq)]
    struct MockError {
        message: String,
    }

    impl std::fmt::Display for MockError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.message)
        }
    }

    impl std::error::Error for MockError {}

    struct MockSttProvider {
        responses: RefCell<VecDeque<Result<String, MockError>>>,
    }

    impl MockSttProvider {
        fn new(responses: Vec<Result<String, MockError>>) -> Self {
            Self {
                responses: RefCell::new(responses.into()),
            }
        }
    }

    impl SttProviderClient<&str, String, MockError> for MockSttProvider {
        fn transcribe_audio(&self, _request: &str) -> Result<String, MockError> {
            self.responses.borrow_mut().pop_front().unwrap_or_else(|| {
                Err(MockError {
                    message: "No more responses".to_string(),
                })
            })
        }
    }

    #[test]
    fn test_get_next_success() {
        let mock_provider = MockSttProvider::new(vec![
            Ok("Success 1".to_string()),
            Ok("Success 2".to_string()),
        ]);

        let requests = vec!["Request 1", "Request 2"];
        let mut queue = TranscriptionQueue::new(&mock_provider, requests);

        assert_eq!(queue.get_next(), Some(Ok("Success 1".to_string())));
        assert_eq!(queue.get_next(), Some(Ok("Success 2".to_string())));
        assert!(queue.get_next().is_none());
    }

    #[test]
    fn test_get_next_with_failure() {
        let mock_provider = MockSttProvider::new(vec![
            Ok("Success 1".to_string()),
            Err(MockError {
                message: "Error occurred".to_string(),
            }),
        ]);

        let requests = vec!["Request 1", "Request 2"];
        let mut queue = TranscriptionQueue::new(&mock_provider, requests);

        // Test get_next

        assert_eq!(queue.get_next(), Some(Ok("Success 1".to_string())));
        assert_eq!(
            queue.get_next(),
            Some(Err(MockError {
                message: "Error occurred".to_string(),
            }))
        );
        assert!(queue.get_next().is_none());
    }

    #[test]
    fn test_blocking_get_next_success() {
        let mock_provider = MockSttProvider::new(vec![
            Ok("Success 1".to_string()),
            Ok("Success 2".to_string()),
            Ok("Success 3".to_string()),
        ]);

        let requests = vec!["Request 1", "Request 2", "Request 3"];
        let mut queue = TranscriptionQueue::new(&mock_provider, requests);

        let actual_results: Vec<String> = queue
            .blocking_get_next()
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        let expected_results = vec![
            "Success 1".to_string(),
            "Success 2".to_string(),
            "Success 3".to_string(),
        ];

        assert_eq!(actual_results, expected_results);
    }

    #[test]
    fn test_blocking_get_next_with_mixed_results() {
        let mock_provider = MockSttProvider::new(vec![
            Ok("Success".to_string()),
            Err(MockError {
                message: "Error occurred".to_string(),
            }),
            Ok("Another success".to_string()),
        ]);

        let requests = vec!["Request 1", "Request 2", "Request 3"];
        let mut queue = TranscriptionQueue::new(&mock_provider, requests);

        let results = queue.blocking_get_next();

        let expected_results = vec![
            Ok("Success".to_string()),
            Err(MockError {
                message: "Error occurred".to_string(),
            }),
            Ok("Another success".to_string()),
        ];

        assert_eq!(results, expected_results);
    }

    #[test]
    fn test_generic_queue_all_errors() {
        let mock_provider = MockSttProvider::new(vec![
            Err(MockError {
                message: "First error".to_string(),
            }),
            Err(MockError {
                message: "Second error".to_string(),
            }),
        ]);

        let requests = vec!["Request 1", "Request 2"];
        let mut queue = TranscriptionQueue::new(&mock_provider, requests);

        let results = queue.blocking_get_next();

        let expected_results = vec![
            Err(MockError {
                message: "First error".to_string(),
            }),
            Err(MockError {
                message: "Second error".to_string(),
            }),
        ];

        assert_eq!(results, expected_results);
    }
}
