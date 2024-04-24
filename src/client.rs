use std::str::FromStr;

use ldk_node::{
    bitcoin::secp256k1::PublicKey, lightning::ln::msgs::SocketAddress,
    lightning_invoice::Bolt11Invoice,
};

use crate::{
    CompactChannel, FundingAddress, GetBalanceResponse, GetInvoiceRequest, GetInvoiceResponse, LspConfig, OpenChannelRequest, OpenChannelResponse, PayInvoiceRequest, PayInvoiceResponse
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
    ) -> Result<OpenChannelResponse, minreq::Error> {
        let url = format!("{}/channels", self.base_url);
        let req = OpenChannelRequest {
            pubkey,
            ip_port: ip_port.to_string(),
            funding_sats,
            push_sats,
        };
        let res = minreq::post(url).with_json(&req).unwrap().send()?;
        let open_channel_response = res.json::<OpenChannelResponse>()?;
        Ok(open_channel_response)
    }

    pub fn pay_invoice(&self, invoice: &Bolt11Invoice) -> Result<String, minreq::Error> {
        let url: String = format!("{}/pay-invoice", self.base_url);
        let req = PayInvoiceRequest {
            invoice: invoice.to_string(),
        };
        let res = minreq::post(url).with_json(&req).unwrap().send()?;
        Ok(res.json::<PayInvoiceResponse>()?.payment_hash)
    }

    pub fn get_invoice(
        &self,
        amount_sats: u64,
        description: &str,
        expiry_secs: u32,
    ) -> Result<Bolt11Invoice, minreq::Error> {
        let url = format!("{}/get-invoice", self.base_url);
        let req = GetInvoiceRequest {
            amount_sats,
            description: description.to_string(),
            expiry_secs,
        };
        let res = minreq::post(url).with_json(&req).unwrap().send()?;
        let invoice_str = res.json::<GetInvoiceResponse>()?.invoice;
        Ok(Bolt11Invoice::from_str(&invoice_str).unwrap())
    }

    pub fn sync(&self) -> Result<(), minreq::Error> {
        let url = format!("{}/sync", self.base_url);
        minreq::post(url).send()?;
        Ok(())
    }

    pub fn get_balance(&self) -> Result<GetBalanceResponse, minreq::Error> {
        let url = format!("{}/balance", self.base_url);
        minreq::get(url).send()?.json::<GetBalanceResponse>()
    }
}
