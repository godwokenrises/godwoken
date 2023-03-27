use std::time::Duration;

use anyhow::Result;
use jsonrpc_utils::HttpClient;
use rand::{thread_rng, Rng};
use reqwest::Client;
use tracing::{field, instrument, Span};

pub(crate) const DEFAULT_QUERY_LIMIT: usize = 500;

pub(crate) type JsonH256 = ckb_fixed_hash::H256;

const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct TracingHttpClient {
    pub(crate) inner: HttpClient,
}

impl TracingHttpClient {
    pub fn with_url(url: String) -> Result<Self> {
        Ok(Self {
            inner: HttpClient::with_client(
                url,
                Client::builder().timeout(DEFAULT_HTTP_TIMEOUT).build()?,
            ),
        })
    }

    pub fn url(&self) -> &str {
        self.inner.url()
    }

    #[instrument(target = "gw-rpc-client", skip_all, fields(method, params = field::Empty))]
    pub async fn rpc(
        &self,
        method: &str,
        params: &serde_json::value::RawValue,
    ) -> Result<serde_json::Value> {
        if params.get().len() < 64 {
            Span::current().record("params", field::display(&params));
        }

        let mut backoff = ExponentialBackoff::new(Duration::from_secs(1));

        loop {
            match self.inner.rpc(method, params).await {
                Ok(r) => return Ok(r),
                Err(e) => {
                    // Retry on reqwest errors. CKB RPCs are almost all safe to retry.
                    if e.is::<reqwest::Error>() {
                        let next = backoff.next_sleep();
                        // reqwest::Error displays the whole chain, no need to use {:#}.
                        tracing::warn!(
                            "rpc client error, will retry in {:.2}s: {}",
                            next.as_secs_f64(),
                            e,
                        );
                        tokio::time::sleep(next).await;
                        continue;
                    }
                    return Err(e.context(format!("rpc {method}")));
                }
            }
        }
    }
}

pub struct ExponentialBackoff {
    base: Duration,
    current_multiplier: f32,
    multiplier: f32,
    max_sleep: Duration,
    jitter: bool,
}

impl ExponentialBackoff {
    pub fn new(base: Duration) -> Self {
        Self {
            base,
            current_multiplier: 1.0,
            multiplier: 2.0,
            max_sleep: base * 32,
            jitter: true,
        }
    }

    pub fn next_sleep(&mut self) -> Duration {
        let t = self.base.mul_f32(self.current_multiplier);
        let t = if t >= self.max_sleep {
            self.max_sleep
        } else {
            self.current_multiplier *= self.multiplier;
            t
        };
        if self.jitter {
            // https://aws.amazon.com/cn/blogs/architecture/exponential-backoff-and-jitter/
            thread_rng().gen_range(Duration::ZERO..t)
        } else {
            t
        }
    }

    pub fn reset(&mut self) {
        self.current_multiplier = 1.0;
    }

    pub fn with_multiplier(self, multiplier: f32) -> Self {
        Self { multiplier, ..self }
    }

    pub fn with_max_sleep(self, max_sleep: Duration) -> Self {
        Self { max_sleep, ..self }
    }

    pub fn with_jitter(self, jitter: bool) -> Self {
        Self { jitter, ..self }
    }
}

#[cfg(test)]
#[test]
fn test_backoff() {
    let mut b =
        ExponentialBackoff::new(Duration::from_secs(1)).with_max_sleep(Duration::from_secs(64));
    b.next_sleep();
    assert!(b.current_multiplier == 2.0);
    b.next_sleep();
    assert!(b.current_multiplier == 4.0);
    for _ in 0..10 {
        b.next_sleep();
    }
    assert!(b.current_multiplier == 64.0);
}
