use std::{num::NonZeroU32, sync::Arc, time::Duration};

use bytes::Bytes;
use governor::{
    Quota, RateLimiter, clock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use reqwest::IntoUrl;
use serde::de::DeserializeOwned;
use thiserror::Error;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct Client {
    client: reqwest::Client,
    limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
    max_retries: u8,
    sem: Arc<Semaphore>,
}

#[derive(Error, Debug)]
pub enum JsonDecodeError {
    #[error("Network error while decoding JSON {0}")]
    NetworkError(#[from] GetError),
    #[error("Decoding error while decoding JSON {0}")]
    DecodeError(#[from] serde_json::Error),
}

#[derive(Error, Debug)]
pub enum GetError {
    #[error("Max retries reached, last error was {0}")]
    MaxRetriesReached(#[from] reqwest::Error),
    #[error("Could not get semaphore permit")]
    SemaphoreError(#[from] tokio::sync::AcquireError),
}

pub enum RequestType {
    Get,
    Post(serde_json::Value),
}

impl Client {
    pub fn new() -> Self {
        Client {
            client: reqwest::Client::new(),
            limiter: None,
            max_retries: 0,
            sem: Arc::new(Semaphore::new(1)),
        }
    }

    pub fn with_limit(mut self, requests_per_second: NonZeroU32) -> Self {
        self.limiter = Some(Arc::new(RateLimiter::direct(Quota::per_second(
            requests_per_second,
        ))));
        self
    }

    pub fn with_max_retries(mut self, max_retries: u8) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub async fn get<U: IntoUrl>(&self, url: U) -> Result<Bytes, GetError> {
        self.get_or_post(url, RequestType::Get).await
    }

    pub async fn post<U: IntoUrl>(
        &self,
        url: U,
        body: serde_json::Value,
    ) -> Result<Bytes, GetError> {
        self.get_or_post(url, RequestType::Post(body)).await
    }

    async fn get_or_post<U: IntoUrl>(
        &self,
        url: U,
        req_type: RequestType,
    ) -> Result<Bytes, GetError> {
        let mut retries = 0;
        let mut err: Option<reqwest::Error> = None;

        let url = url.into_url()?;

        while retries <= self.max_retries {
            let request = match req_type {
                RequestType::Get => self.client.get(url.clone()),
                RequestType::Post(ref body) => self.client.post(url.clone()).json(body),
            };

            // If we do a retry, hold the sempahore permit so that other requests are halted
            // as well
            let permit = self.sem.acquire().await?;
            if retries > 0 {
                println!("Network error occurred, holding permit for 5 minutes");
                tokio::time::sleep(Duration::from_secs(60 * 5)).await;
            }
            drop(permit);

            match &self.limiter {
                None => (),
                Some(limiter) => limiter.until_ready().await,
            }

            //println!("[{}:{:?}] Fetching {}", retries, &err, &url);

            let response = match request.send().await {
                Ok(response) => match response.error_for_status() {
                    Ok(response) => response,
                    Err(e) => {
                        err = Some(e);
                        retries += 1;
                        continue;
                    }
                },
                Err(e) => {
                    err = Some(e);
                    retries += 1;
                    continue;
                }
            };

            match response.bytes().await {
                Ok(res) => return Ok(res),
                Err(e) => {
                    err = Some(e);
                    retries += 1;
                    continue;
                }
            }
        }
        Err(err.unwrap())?
    }

    pub async fn get_json<U: IntoUrl, T: DeserializeOwned>(
        &self,
        url: U,
    ) -> Result<T, JsonDecodeError> {
        let response = self.get(url).await?;
        serde_json::from_slice(&response).map_err(JsonDecodeError::DecodeError)
    }

    pub async fn get_json_post<U: IntoUrl, T: DeserializeOwned>(
        &self,
        url: U,
        body: serde_json::Value,
    ) -> Result<T, JsonDecodeError> {
        let response = self.post(url, body).await?;
        serde_json::from_slice(&response).map_err(JsonDecodeError::DecodeError)
    }
}
