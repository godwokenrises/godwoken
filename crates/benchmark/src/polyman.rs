use std::fmt::Debug;

use anyhow::Result;
use ckb_fixed_hash::H256;
use ckb_jsonrpc_types::{Uint128, Uint32};
use serde::{Deserialize, Serialize};

use reqwest::{Client, Url};

#[derive(Clone)]
pub struct PolymanClient {
    client: Client,
    url: Url,
}

impl PolymanClient {
    pub fn new(url: Url) -> Self {
        Self {
            client: Client::new(),
            url,
        }
    }

    pub async fn deploy(&self) -> Result<Response<BuildDeployResponse>> {
        let res = self
            .client
            .get(self.url.clone().join("build_deploy")?)
            .send()
            .await?;
        let res: Response<BuildDeployResponse> = res.json().await?;
        Ok(res)
    }

    pub async fn build_erc20(
        &self,
        from_id: u32,
        to_id: u32,
        amount: u128,
    ) -> Result<Response<BuildErc20Response>> {
        let from_id = format!("{:#x}", from_id);
        let to_id = format!("{:#x}", to_id);
        let amount = format!("{}", amount);
        let res = self
            .client
            .get(self.url.clone().join("build_transfer")?)
            .query(&[("amount", amount), ("from_id", from_id), ("to_id", to_id)])
            .build()?;
        let res = self.client.execute(res).await?;
        Ok(res.json().await?)
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct BuildDeployResponse {
    pub proxy_contract_id: Uint32,
    pub proxy_contract_script_hash: H256,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct BuildErc20Request {
    from_id: Uint32,
    to_id: Uint32,
    amount: Uint128,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct BuildErc20Response {
    pub args: String,
    pub nonce: Uint32,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct Response<T> {
    pub status: Status,
    pub error: Option<String>,
    pub data: Option<T>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]

pub enum Status {
    Ok,
    Failed,
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::PolymanClient;

    #[tokio::test]
    pub async fn test_build_deploy() -> Result<()> {
        let url = reqwest::Url::parse("http://localhost:6101")?;
        let client = PolymanClient::new(url);
        let res = client.deploy().await?;
        println!("res: {:?}", res);
        Ok(())
    }

    #[tokio::test]
    pub async fn test_build_erc20() -> Result<()> {
        let url = reqwest::Url::parse("http://localhost:6101")?;
        let client = PolymanClient::new(url);
        let res = client.build_erc20(18, 19, 20000000000).await?;
        println!("res: {:?}", res);

        Ok(())
    }
}
