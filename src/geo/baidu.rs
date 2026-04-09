use anyhow::{bail, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::coord::wgs84_to_bd09;
use super::provider::{CoordSystem, GeocodeResult, GeoProvider};

pub struct BaiduProvider {
    ak: String,
    client: Client,
}

impl BaiduProvider {
    pub fn new(ak: String) -> Self {
        Self {
            ak,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build reqwest client"),
        }
    }
}

#[async_trait]
impl GeoProvider for BaiduProvider {
    fn name(&self) -> &str {
        "baidu"
    }

    fn coord_system(&self) -> CoordSystem {
        CoordSystem::Bd09
    }

    async fn reverse_geocode(&self, lat: f64, lng: f64) -> Result<GeocodeResult> {
        let (b_lat, b_lng) = wgs84_to_bd09(lat, lng);
        let location = format!("{:.6},{:.6}", b_lat, b_lng);

        let resp: BaiduResponse = self
            .client
            .get("https://api.map.baidu.com/reverse_geocoding/v3/")
            .query(&[
                ("location", location.as_str()),
                ("output", "json"),
                ("coordtype", "bd09ll"),
                ("extensions_poi", "0"),
                ("ak", self.ak.as_str()),
            ])
            .send()
            .await?
            .json()
            .await?;

        if resp.status != 0 {
            bail!(
                "Baidu API error: status={}, message={}",
                resp.status,
                resp.message.as_deref().unwrap_or("unknown")
            );
        }

        let result = resp.result.ok_or_else(|| anyhow::anyhow!("Baidu: empty result"))?;
        let raw = serde_json::to_value(&result)?;
        let comp = result.address_component;

        debug!(address = %result.formatted_address, "Baidu geocode OK");

        Ok(GeocodeResult {
            display_name: result.formatted_address,
            city: comp.city,
            county: comp.district,
            province: comp.province,
            country: Some("中国".into()),
            postcode: None,
            name: None,
            house_number: comp.street_number,
            road: comp.street,
            neighbourhood: None,
            latitude: None,
            longitude: None,
            raw: raw,
            state_district: None,
        })
    }
}

// ---- Response types ----

#[derive(Deserialize)]
struct BaiduResponse {
    status: i32,
    message: Option<String>,
    result: Option<BaiduResult>,
}

#[derive(Deserialize, Serialize)]
struct BaiduResult {
    formatted_address: String,
    address_component: BaiduAddressComponent,
}

#[derive(Deserialize, Serialize)]
struct BaiduAddressComponent {
    province: Option<String>,
    city: Option<String>,
    district: Option<String>,
    street: Option<String>,
    street_number: Option<String>,
    town: Option<String>,
}
