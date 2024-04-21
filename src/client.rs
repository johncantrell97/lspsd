use crate::{FundingAddress, LspConfig};

#[derive(Debug)]
pub struct LspsClient {
    base_url: String,
}

impl LspsClient {
    pub fn new(base_url: &str) -> Self {
        LspsClient {
            base_url: base_url.to_string(),
        }
    }

    pub fn get_lsps_config(&self) -> Result<LspConfig, minreq::Error> {
        let url = format!("{}/config", self.base_url);
        minreq::get(url).send()?.json::<LspConfig>()
    }

    pub fn get_funding_address(&self) -> Result<FundingAddress, minreq::Error> {
        let url = format!("{}/funding-address", self.base_url);
        minreq::get(url).send()?.json::<FundingAddress>()
    }
}
