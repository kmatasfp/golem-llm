use crate::exports::golem::web_search::web_search::Guest;
use crate::exports::golem::web_search::web_search::{
    SearchError, SearchMetadata, SearchParams, SearchResult,
};
use golem_rust::value_and_type::{FromValueAndType, IntoValue as IntoValueTrait};
use std::marker::PhantomData;

/// Wraps a websearch implementation with custom durability
pub struct Durablewebsearch<Impl> {
    phantom: PhantomData<Impl>,
}

/// Trait to be implemented in addition to the websearch `Guest` trait when wrapping it with `Durablewebsearch`.
pub trait ExtendedwebsearchGuest: Guest + 'static {
    /// Internal, provider specific state that fully captures current search session + current search results + current search metadata
    type ReplayState: std::fmt::Debug + Clone + IntoValueTrait + FromValueAndType;

    /// Creates an instance of the websearch specific `SearchSession` without wrapping it in a `Resource`
    fn unwrapped_search_session(params: SearchParams) -> Result<Self::SearchSession, SearchError>;

    /// Used at the end of replay to go from replay to live mode
    fn session_from_state(state: &Self::ReplayState) -> Self::SearchSession;

    /// Used in live mode to record states that can be used for replay
    fn session_to_state(session: &Self::SearchSession) -> Self::ReplayState;

    /// Get the current search results from the state
    fn search_result_from_state(state: &Self::ReplayState) -> Vec<SearchResult>;

    /// Get the current search metadata from the state
    fn search_metadata_from_state(state: &Self::ReplayState) -> Option<SearchMetadata>;

    /// Creates the retry prompt with a combination of the original search params, and the partially received
    /// search results. There is a default implementation here, but it can be overridden with provider-specific
    /// parameters if needed.
    fn retry_params(
        original_params: &SearchParams,
        partial_results: &[SearchResult],
    ) -> SearchParams {
        // For search, we typically want to continue from where we left off
        // This could involve adjusting max_results or using pagination tokens
        let mut retry_params = original_params.clone();

        if let Some(max_results) = retry_params.max_results {
            // Reduce max_results by the number of results we already have
            let remaining = max_results.saturating_sub(partial_results.len() as u32);
            retry_params.max_results = Some(remaining.max(1));
        }

        retry_params
    }
}

/// When the durability feature flag is off, wrapping with `Durablewebsearch` is just a passthrough
#[cfg(not(feature = "durability"))]
mod passthrough_impl {
    use crate::durability::{Durablewebsearch, ExtendedwebsearchGuest};
    use crate::golem::web_search::web_search::{Guest, SearchSession};
    use crate::golem::web_search::web_search::{
        SearchError, SearchMetadata, SearchParams, SearchResult,
    };

    impl<Impl: ExtendedwebsearchGuest> Guest for Durablewebsearch<Impl> {
        type SearchSession = Impl::SearchSession;

        fn start_search(params: SearchParams) -> Result<SearchSession, SearchError> {
            Impl::start_search(params)
        }

        fn search_once(
            params: SearchParams,
        ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
            Impl::search_once(params)
        }
    }
}

/// When the durability feature flag is on, wrapping with `Durablewebsearch` adds custom durability
/// on top of the provider-specific websearch implementation using Golem's special host functions and
/// the `golem-rust` helper library.
///
/// There will be custom durability entries saved in the oplog, with the full websearch request and configuration
/// stored as input, and the full response stored as output. To serialize these in a way it is
/// observable by oplog consumers, each relevant data type has to be converted to/from `ValueAndType`
/// which is implemented using the type classes and builder in the `golem-rust` library.
#[cfg(feature = "durability")]
mod durable_impl {
    use crate::durability::{Durablewebsearch, ExtendedwebsearchGuest};
    use crate::exports::golem::web_search::web_search::{Guest, GuestSearchSession, SearchSession};
    use crate::exports::golem::web_search::web_search::{
        SearchError, SearchMetadata, SearchParams, SearchResult,
    };
    use golem_rust::bindings::golem::durability::durability::DurableFunctionType;
    use golem_rust::durability::Durability;
    use golem_rust::{with_persistence_level, PersistenceLevel};
    use std::cell::RefCell;

    // Add the From implementation for SearchError to satisfy the Durability trait bounds
    impl From<&SearchError> for SearchError {
        fn from(error: &SearchError) -> Self {
            error.clone()
        }
    }

    impl<Impl: ExtendedwebsearchGuest> Guest for Durablewebsearch<Impl> {
        type SearchSession = DurableSearchSession<Impl>;

        fn start_search(params: SearchParams) -> Result<SearchSession, SearchError> {
            let durability = Durability::<SearchParams, SearchError>::new(
                "golem_websearch",
                "start_search",
                DurableFunctionType::WriteRemote,
            );

            if durability.is_live() {
                let result = with_persistence_level(PersistenceLevel::PersistNothing, || {
                    match Impl::start_search(params.clone()) {
                        Ok(_session) => Ok(params.clone()),
                        Err(e) => Err(e),
                    }
                });

                match result {
                    Ok(persisted_params) => {
                        durability
                            .persist(persisted_params.clone(), Ok(persisted_params.clone()))?;
                        Ok(SearchSession::new(DurableSearchSession::<Impl>::live(
                            Impl::unwrapped_search_session(persisted_params).unwrap(),
                        )))
                    }
                    Err(error) => {
                        durability.persist(params.clone(), Err(error.clone()))?;
                        Err(error)
                    }
                }
            } else {
                let result = durability.replay::<SearchParams, SearchError>()?;
                let session = SearchSession::new(DurableSearchSession::<Impl>::replay(result));
                Ok(session)
            }
        }

        fn search_once(
            params: SearchParams,
        ) -> Result<(Vec<SearchResult>, Option<SearchMetadata>), SearchError> {
            let durability =
                Durability::<(Vec<SearchResult>, Option<SearchMetadata>), SearchError>::new(
                    "golem_websearch",
                    "search_once",
                    DurableFunctionType::WriteRemote,
                );

            if durability.is_live() {
                let result = with_persistence_level(PersistenceLevel::PersistNothing, || {
                    Impl::search_once(params.clone())
                });

                match result {
                    Ok((results, metadata)) => {
                        durability
                            .persist(params.clone(), Ok((results.clone(), metadata.clone())))?;
                        Ok((results, metadata))
                    }
                    Err(error) => {
                        let _ = durability
                            .persist::<_, (Vec<SearchResult>, Option<SearchMetadata>), SearchError>(
                                params.clone(),
                                Err(error.clone()),
                            );
                        Err(error)
                    }
                }
            } else {
                let result = durability
                    .replay::<(Vec<SearchResult>, Option<SearchMetadata>), SearchError>()?;
                Ok(result)
            }
        }
    }

    /// Represents the durable search session's state
    ///
    /// In live mode it directly calls the underlying websearch session which is implemented on
    /// top of HTTP requests to search providers.
    ///
    /// In replay mode it uses the replay state to reconstruct the session state accurately,
    /// tracking accumulated results and metadata.
    ///
    /// When reaching the end of the replay mode, if the replayed session was not finished yet,
    /// the retry parameters implemented in `ExtendedwebsearchGuest` is used to create a new websearch session
    /// and continue the search seamlessly.
    enum DurableSearchSessionState<Impl: ExtendedwebsearchGuest> {
        Live {
            session: Impl::SearchSession,
        },
        Replay {
            current_state: Impl::ReplayState,
            original_params: SearchParams,
            current_page: u32,
            finished: bool,
        },
    }

    pub struct DurableSearchSession<Impl: ExtendedwebsearchGuest> {
        state: RefCell<Option<DurableSearchSessionState<Impl>>>,
    }

    impl<Impl: ExtendedwebsearchGuest> DurableSearchSession<Impl> {
        fn live(session: Impl::SearchSession) -> Self {
            Self {
                state: RefCell::new(Some(DurableSearchSessionState::Live { session })),
            }
        }

        fn replay(original_params: SearchParams) -> Self {
            // Initialize with empty state - will be populated during replay
            let current_state = Impl::session_to_state(
                &Impl::unwrapped_search_session(original_params.clone()).unwrap(),
            );

            Self {
                state: RefCell::new(Some(DurableSearchSessionState::Replay {
                    current_state,
                    original_params,
                    current_page: 0,
                    finished: false,
                })),
            }
        }
    }

    impl<Impl: ExtendedwebsearchGuest> Drop for DurableSearchSession<Impl> {
        fn drop(&mut self) {
            match self.state.take() {
                Some(DurableSearchSessionState::Live { session }) => {
                    with_persistence_level(PersistenceLevel::PersistNothing, move || {
                        drop(session);
                    });
                }
                Some(DurableSearchSessionState::Replay { .. }) => {
                    // Nothing special to clean up for replay state
                }
                None => {}
            }
        }
    }

    impl<Impl: ExtendedwebsearchGuest> GuestSearchSession for DurableSearchSession<Impl> {
        fn next_page(&self) -> Result<Vec<SearchResult>, SearchError> {
            let durability = Durability::<Vec<SearchResult>, SearchError>::new(
                "golem_websearch",
                "next_page",
                DurableFunctionType::ReadRemote,
            );

            if durability.is_live() {
                let mut state = self.state.borrow_mut();
                let (result, new_live_session) = match &mut *state {
                    Some(DurableSearchSessionState::Live { session }) => {
                        let result =
                            with_persistence_level(PersistenceLevel::PersistNothing, || {
                                session.next_page()
                            });

                        match result {
                            Ok(value) => {
                                let persisted_result =
                                    durability.persist((0u8, 0u8), Ok(value.clone()))?;
                                (Ok(persisted_result), None)
                            }
                            Err(error) => {
                                let _ = durability.persist::<_, Vec<SearchResult>, SearchError>(
                                    (0u8, 0u8),
                                    Err(error.clone()),
                                );
                                (Err(error), None)
                            }
                        }
                    }
                    Some(DurableSearchSessionState::Replay {
                        current_state,
                        original_params,
                        current_page,
                        finished,
                    }) => {
                        *current_page += 1;

                        if *finished {
                            let empty_result = durability.persist((0u8, 0u8), Ok(Vec::new()))?;
                            (Ok(empty_result), None)
                        } else {
                            // Get current partial results from state
                            let partial_results = Impl::search_result_from_state(current_state);
                            let retry_params =
                                Impl::retry_params(original_params, &partial_results);

                            let (session, first_live_result) =
                                with_persistence_level(PersistenceLevel::PersistNothing, || {
                                    let session = Impl::unwrapped_search_session(retry_params)?;
                                    let next = session.next_page();
                                    Ok::<
                                        (
                                            Impl::SearchSession,
                                            Result<Vec<SearchResult>, SearchError>,
                                        ),
                                        SearchError,
                                    >((session, next))
                                })?;

                            match first_live_result {
                                Ok(value) => {
                                    let persisted_result =
                                        durability.persist((0u8, 0u8), Ok(value.clone()))?;
                                    (Ok(persisted_result), Some(session))
                                }
                                Err(error) => {
                                    let _ = durability
                                        .persist::<_, Vec<SearchResult>, SearchError>(
                                            (0u8, 0u8),
                                            Err(error.clone()),
                                        );
                                    (Err(error), Some(session))
                                }
                            }
                        }
                    }
                    None => {
                        unreachable!()
                    }
                };

                if let Some(session) = new_live_session {
                    *state = Some(DurableSearchSessionState::Live { session });
                }

                result
            } else {
                let result = durability.replay::<Vec<SearchResult>, SearchError>()?;
                let mut state = self.state.borrow_mut();

                match &mut *state {
                    Some(DurableSearchSessionState::Live { .. }) => {
                        unreachable!("Durable search session cannot be in live mode during replay");
                    }
                    Some(DurableSearchSessionState::Replay { current_page, .. }) => {
                        *current_page += 1;
                        // Update current_state to include the new results
                        // This would need to be implemented by the provider to merge results into state
                        // For now, we'll return the replayed result
                        Ok(result)
                    }
                    None => {
                        unreachable!();
                    }
                }
            }
        }

        fn get_metadata(&self) -> Option<SearchMetadata> {
            let state = self.state.borrow();
            match &*state {
                Some(DurableSearchSessionState::Live { session }) => {
                    // Always delegate to the underlying live session
                    with_persistence_level(PersistenceLevel::PersistNothing, || {
                        session.get_metadata()
                    })
                }
                Some(DurableSearchSessionState::Replay {
                    current_state,
                    current_page,
                    ..
                }) => {
                    // Get metadata from the current replay state and update current_page
                    let mut metadata = Impl::search_metadata_from_state(current_state);
                    if let Some(ref mut meta) = metadata {
                        meta.current_page = *current_page;
                    }
                    metadata
                }
                None => {
                    unreachable!()
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use crate::golem::web_search::types::{
            ImageResult, RateLimitInfo, SafeSearchLevel, TimeRange,
        };
        use crate::golem::web_search::web_search::{
            SearchError, SearchMetadata, SearchParams, SearchResult,
        };
        use golem_rust::value_and_type::{FromValueAndType, IntoValueAndType};
        use golem_rust::wasm_rpc::WitTypeNode;
        use std::fmt::Debug;

        fn roundtrip_test<T: Debug + Clone + PartialEq + IntoValueAndType + FromValueAndType>(
            value: T,
        ) {
            let vnt = value.clone().into_value_and_type();
            let extracted = T::from_value_and_type(vnt).unwrap();
            assert_eq!(value, extracted);
        }

        #[test]
        fn safe_search_level_roundtrip() {
            roundtrip_test(SafeSearchLevel::Off);
            roundtrip_test(SafeSearchLevel::Medium);
            roundtrip_test(SafeSearchLevel::High);
        }

        #[test]
        fn time_range_roundtrip() {
            roundtrip_test(TimeRange::Day);
            roundtrip_test(TimeRange::Week);
            roundtrip_test(TimeRange::Month);
            roundtrip_test(TimeRange::Year);
        }

        #[test]
        fn search_error_roundtrip() {
            roundtrip_test(SearchError::InvalidQuery);
            roundtrip_test(SearchError::RateLimited(3600));
            roundtrip_test(SearchError::UnsupportedFeature(
                "advanced search".to_string(),
            ));
            roundtrip_test(SearchError::BackendError("Service unavailable".to_string()));
        }

        #[test]
        fn image_result_roundtrip() {
            roundtrip_test(ImageResult {
                url: "https://example.com/image.png".to_string(),
                description: Some("A sample image".to_string()),
            });
            roundtrip_test(ImageResult {
                url: "https://example.com/image2.jpg".to_string(),
                description: None,
            });
        }

        #[test]
        fn rate_limit_info_roundtrip() {
            roundtrip_test(RateLimitInfo {
                limit: 1000,
                remaining: 500,
                reset_timestamp: 1698761200,
            });
        }

        #[test]
        fn search_result_roundtrip() {
            roundtrip_test(SearchResult {
                title: "Sample Search Result".to_string(),
                url: "https://example.com/page".to_string(),
                snippet: "This is a sample search result snippet".to_string(),
                display_url: Some("example.com/page".to_string()),
                source: Some("Example Website".to_string()),
                score: Some(0.95),
                html_snippet: Some("<p>This is a sample search result snippet</p>".to_string()),
                date_published: Some("2023-10-01".to_string()),
                images: Some(vec![ImageResult {
                    url: "https://example.com/thumb.jpg".to_string(),
                    description: Some("Thumbnail".to_string()),
                }]),
                content_chunks: Some(vec![
                    "First chunk of content".to_string(),
                    "Second chunk of content".to_string(),
                ]),
            });
        }

        #[test]
        fn search_metadata_roundtrip() {
            roundtrip_test(SearchMetadata {
                query: "sample search query".to_string(),
                total_results: Some(1500),
                search_time_ms: Some(125.5),
                safe_search: Some(SafeSearchLevel::Medium),
                language: Some("en".to_string()),
                region: Some("US".to_string()),
                next_page_token: Some("next_page_123".to_string()),
                rate_limits: Some(RateLimitInfo {
                    limit: 1000,
                    remaining: 999,
                    reset_timestamp: 1698761200,
                }),
                current_page: 0,
            });
        }

        #[test]
        fn search_params_roundtrip() {
            roundtrip_test(SearchParams {
                query: "rust programming language".to_string(),
                safe_search: Some(SafeSearchLevel::High),
                language: Some("en".to_string()),
                region: Some("US".to_string()),
                max_results: Some(50),
                time_range: Some(TimeRange::Month),
                include_domains: Some(vec![
                    "rust-lang.org".to_string(),
                    "doc.rust-lang.org".to_string(),
                ]),
                exclude_domains: Some(vec!["spam.com".to_string()]),
                include_images: Some(true),
                include_html: Some(false),
                advanced_answer: Some(true),
            });
        }

        #[test]
        fn start_search_input_encoding() {
            let input = SearchParams {
                query: "machine learning tutorials".to_string(),
                safe_search: Some(SafeSearchLevel::Medium),
                language: Some("en".to_string()),
                region: Some("US".to_string()),
                max_results: Some(25),
                time_range: Some(TimeRange::Week),
                include_domains: Some(vec![
                    "github.com".to_string(),
                    "stackoverflow.com".to_string(),
                ]),
                exclude_domains: Some(vec!["ads.com".to_string()]),
                include_images: Some(true),
                include_html: Some(true),
                advanced_answer: Some(false),
            };

            let encoded = input.into_value_and_type();
            println!("{encoded:#?}");

            for wit_type in encoded.typ.nodes {
                if let WitTypeNode::ListType(idx) = wit_type {
                    assert!(idx >= 0);
                }
            }
        }

        #[test]
        fn search_once_input_encoding() {
            let input = SearchParams {
                query: "web development best practices".to_string(),
                safe_search: Some(SafeSearchLevel::Off),
                language: Some("en".to_string()),
                region: Some("GB".to_string()),
                max_results: Some(10),
                time_range: None,
                include_domains: None,
                exclude_domains: None,
                include_images: Some(false),
                include_html: Some(true),
                advanced_answer: Some(true),
            };

            let encoded = input.into_value_and_type();
            println!("{encoded:#?}");

            for wit_type in encoded.typ.nodes {
                if let WitTypeNode::ListType(idx) = wit_type {
                    assert!(idx >= 0);
                }
            }
        }
    }
}
