use ldk_node::{
    bitcoin::secp256k1::PublicKey,
    lightning::ln::msgs::SocketAddress,
    lightning_invoice::Bolt11Invoice,
};

use crate::{
    CompactChannel, FundingAddress, LspConfig, OpenChannelRequest, OpenChannelResponse,
    PayInvoiceRequest, PayInvoiceResponse,
};

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

    pub fn list_channels(&self) -> Result<Vec<CompactChannel>, minreq::Error> {
        let url = format!("{}/channels", self.base_url);
        minreq::get(url).send()?.json::<Vec<CompactChannel>>()
    }

    pub fn open_channel(
        &self,
        pubkey: PublicKey,
        ip_port: SocketAddress,
        funding_sats: u64,
        push_sats: u64,
    ) -> Result<u128, minreq::Error> {
        let url = format!("{}/open-channel", self.base_url);
        let req = OpenChannelRequest {
            pubkey,
            ip_port: ip_port.to_string(),
            funding_sats,
            push_sats,
        };
        let res = minreq::post(url).with_json(&req).unwrap().send()?;
        Ok(res.json::<OpenChannelResponse>()?.user_channel_id)
    }

    pub fn pay_invoice(&self, invoice: &Bolt11Invoice) -> Result<String, minreq::Error> {
        let url = format!("{}/invoices", self.base_url);
        let req = PayInvoiceRequest {
            invoice: invoice.to_string(),
        };
        let res = minreq::post(url).with_json(&req).unwrap().send()?;
        Ok(res.json::<PayInvoiceResponse>()?.payment_hash)
    }
}
